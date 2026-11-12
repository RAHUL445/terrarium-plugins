use quick_xml::{events::Event, Reader};
use serde_json::{json, Map, Value};
use terrarium_guest::{clock, http, storage, Args, Error, Plugin, Result};

struct Feeds;
terrarium_guest::plugin!(Feeds);

impl Plugin for Feeds {
    fn handle(op: &str, args: Args) -> Result<Value> {
        match op {
            "add" => {
                let url = feed_url(args.str("url")?)?;
                let rows = storage::query(
                    "SELECT id FROM feeds WHERE url = ?1 AND _deleted IS NULL LIMIT 1",
                    &[json!(url)],
                )?;
                if let Some(row) = rows.first() {
                    return Ok(json!({"id": row["id"], "existing": true}));
                }
                let id = storage::insert(
                    "feeds",
                    &values(json!({"url":url,"title":url,"paused":false})),
                )?;
                fetch(&values(json!({"id":id,"url":url})))?;
                Ok(json!({"id":id}))
            }
            "refresh" | "scheduled_refresh" => {
                let rows = storage::query("SELECT * FROM feeds WHERE _deleted IS NULL AND paused = 0 AND (?1 IS NULL OR ?1 = '' OR id = ?1) ORDER BY COALESCE(fetched_ms, 0), id LIMIT 20", &[json!(args.opt_str("id"))])?;
                for row in &rows {
                    fetch(row)?;
                }
                Ok(json!({"queued": rows.len()}))
            }
            "on_fetched" => on_fetched(args),
            "remove" => {
                let id = args.str("id")?;
                for row in storage::query(
                    "SELECT id FROM items WHERE feed_id = ?1 AND _deleted IS NULL",
                    &[json!(id)],
                )? {
                    storage::delete("items", row["id"].as_str().unwrap())?;
                }
                // A repeat remove (or a late reply) cannot recreate the feed.
                if !storage::query(
                    "SELECT id FROM feeds WHERE id = ?1 AND _deleted IS NULL",
                    &[json!(id)],
                )?
                .is_empty()
                {
                    storage::delete("feeds", id)?;
                }
                Ok(json!({}))
            }
            "mark_read" => {
                storage::update(
                    "items",
                    args.str("id")?,
                    &values(json!({"read":args.bool("read")?})),
                )?;
                Ok(json!({}))
            }
            "mark_all_read" => {
                for row in storage::query("SELECT id FROM items WHERE _deleted IS NULL AND read = 0 AND (?1 IS NULL OR ?1 = '' OR feed_id = ?1)", &[json!(args.opt_str("feed_id"))])? {
                    storage::update("items", row["id"].as_str().unwrap(), &values(json!({"read":true})))?;
                }
                Ok(json!({}))
            }
            _ => Err(Error::unknown_operation(op)),
        }
    }
}

fn values(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn feed_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.chars().any(|c| c.is_control() || c == '\\') {
        return Err(Error::invalid("Enter a public HTTPS feed address"));
    }
    let url =
        url::Url::parse(raw).map_err(|_| Error::invalid("Enter a public HTTPS feed address"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port().is_some_and(|p| p != 443)
        || !matches!(url.host(), Some(url::Host::Domain(_)))
    {
        return Err(Error::invalid("Enter a public HTTPS feed address"));
    }
    // The host applies the canonical public-host and DNS policy on submission/dispatch.
    Ok(url.into())
}

fn fetch(row: &Map<String, Value>) -> Result<()> {
    let mut headers = Map::new();
    headers.insert(
        "Accept".into(),
        json!("application/rss+xml, application/atom+xml, application/xml, text/xml"),
    );
    for (column, header) in [
        ("etag", "If-None-Match"),
        ("last_modified", "If-Modified-Since"),
    ] {
        if let Some(v) = row
            .get(column)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            headers.insert(header.into(), json!(v));
        }
    }
    http::fetch(
        &json!({"method":"GET","url":row["url"],"headers":headers,"context":{"feed":row["id"]}}),
        "on_fetched",
    )?;
    Ok(())
}

fn on_fetched(args: Args) -> Result<Value> {
    let id = args
        .get("context")
        .and_then(|c| c.get("feed"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("Missing feed"))?;
    if storage::query(
        "SELECT id FROM feeds WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?
    .is_empty()
    {
        return Ok(json!({}));
    }
    let now = clock::now_ms() as i64;
    let error = if args.get("ok").and_then(Value::as_bool) != Some(true) {
        Some(
            args.get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("Could not fetch the feed")
                .to_string(),
        )
    } else {
        match args.get("status").and_then(Value::as_u64) {
            Some(304) => {
                storage::update(
                    "feeds",
                    id,
                    &values(json!({"fetched_ms":now,"last_error":""})),
                )?;
                return Ok(json!({}));
            }
            Some(200) => None,
            Some(status) => Some(format!("The website returned {status}")),
            None => Some("Could not fetch the feed".into()),
        }
    };
    if let Some(error) = error {
        storage::update(
            "feeds",
            id,
            &values(json!({"last_error":error,"fetched_ms":now})),
        )?;
        return Ok(json!({}));
    }
    let parsed = match parse_feed(args.opt_str("body").unwrap_or(""), now) {
        Ok(feed) => feed,
        Err(_) => {
            storage::update(
                "feeds",
                id,
                &values(
                    json!({"last_error":"Could not read this RSS or Atom feed","fetched_ms":now}),
                ),
            )?;
            return Ok(json!({}));
        }
    };
    for item in parsed.items {
        // Include tombstones: pruning a read item must not resurrect it on refresh.
        if !storage::query(
            "SELECT id FROM items WHERE feed_id = ?1 AND guid = ?2 LIMIT 1",
            &[json!(id), json!(item.guid)],
        )?
        .is_empty()
        {
            continue;
        }
        storage::insert(
            "items",
            &values(
                json!({"feed_id":id,"guid":item.guid,"title":item.title,"link":item.link,"summary":item.summary,"published_ms":item.published_ms,"read":false}),
            ),
        )?;
    }
    // Keep the newest 200, plus older unread items so refreshing never loses reading work.
    for row in storage::query("SELECT id FROM items WHERE feed_id = ?1 AND _deleted IS NULL AND read = 1 AND id NOT IN (SELECT id FROM items WHERE feed_id = ?1 AND _deleted IS NULL ORDER BY published_ms DESC, id DESC LIMIT 200)", &[json!(id)])? {
        storage::delete("items", row["id"].as_str().unwrap())?;
    }
    let mut update = values(
        json!({"fetched_ms":now,"last_error":"","etag":args.get("headers").and_then(|h| h.get("etag")).and_then(Value::as_str).unwrap_or(""),"last_modified":args.get("headers").and_then(|h| h.get("last-modified")).and_then(Value::as_str).unwrap_or("")}),
    );
    if !parsed.title.is_empty() {
        update.insert("title".into(), json!(parsed.title));
    }
    storage::update("feeds", id, &update)?;
    Ok(json!({}))
}

#[derive(Default, Debug)]
struct Feed {
    title: String,
    items: Vec<Item>,
}
#[derive(Default, Debug)]
struct Item {
    title: String,
    link: String,
    guid: String,
    summary: String,
    date: String,
    published_ms: i64,
}

fn safe_link(raw: &str) -> String {
    let raw = raw.trim();
    if raw.chars().any(|c| c.is_control() || c == '\\') {
        return String::new();
    }
    url::Url::parse(raw)
        .ok()
        .filter(|u| {
            matches!(u.scheme(), "https" | "http")
                && u.host().is_some()
                && u.username().is_empty()
                && u.password().is_none()
        })
        .map(String::from)
        .unwrap_or_default()
}

// Feed markup is never handed to the Markdown/HTML renderer. Even malformed
// HTML and script text become inert plain text in the standard item node.
fn plain(raw: &str) -> String {
    let raw = quick_xml::escape::unescape(raw).unwrap_or(std::borrow::Cow::Borrowed(raw));
    let mut text = String::new();
    let mut tag = false;
    let mut quote = None;
    for c in raw.chars() {
        if tag {
            match (quote, c) {
                (None, '\'' | '"') => quote = Some(c),
                (Some(q), c) if q == c => quote = None,
                (None, '>') => {
                    tag = false;
                    text.push(' ');
                }
                _ => {}
            }
        } else if c == '<' {
            tag = true;
        } else {
            text.push(c);
        }
    }
    let text = text.replace("&nbsp;", " ");
    let decoded = quick_xml::escape::unescape(&text).unwrap_or(std::borrow::Cow::Borrowed(&text));
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_feed(body: &str, now: i64) -> std::result::Result<Feed, ()> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().expand_empty_elements = true;
    let mut path: Vec<String> = Vec::new();
    let mut feed = Feed::default();
    let mut item: Option<Item> = None;
    let mut field: Option<(String, usize, String)> = None;
    let mut recognized = false;
    loop {
        match reader.read_event().map_err(|_| ())? {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                path.push(name.clone());
                if path.len() == 1 {
                    if !matches!(name.as_str(), "rss" | "feed") {
                        return Err(());
                    }
                    recognized = true;
                }
                let is_item = path == ["rss", "channel", "item"] || path == ["feed", "entry"];
                if is_item {
                    item = Some(Item::default());
                }
                let parent = path.get(path.len().saturating_sub(2)).map(String::as_str);
                let direct = item.is_some() && matches!(parent, Some("item" | "entry"));
                let feed_title =
                    item.is_none() && name == "title" && matches!(parent, Some("channel" | "feed"));
                if field.is_none()
                    && (feed_title
                        || (direct
                            && matches!(
                                name.as_str(),
                                "title"
                                    | "link"
                                    | "guid"
                                    | "id"
                                    | "description"
                                    | "summary"
                                    | "pubDate"
                                    | "updated"
                                    | "published"
                            )))
                {
                    field = Some((name.clone(), path.len(), String::new()));
                }
                if direct && name == "link" && parent == Some("entry") {
                    let mut href = String::new();
                    let mut rel = String::new();
                    for a in e.attributes() {
                        let a = a.map_err(|_| ())?;
                        let value = a
                            .decoded_and_normalized_value(
                                quick_xml::XmlVersion::Implicit1_0,
                                reader.decoder(),
                            )
                            .map_err(|_| ())?
                            .into_owned();
                        match a.key.as_ref() {
                            b"href" => href = value,
                            b"rel" => rel = value,
                            _ => {}
                        }
                    }
                    if rel.is_empty() || rel == "alternate" {
                        item.as_mut().unwrap().link = safe_link(&href);
                    }
                }
                if path.len() > 64 {
                    return Err(());
                }
            }
            Event::Text(e) => {
                if let Some((_, _, text)) = &mut field {
                    text.push_str(&e.decode().map_err(|_| ())?);
                }
            }
            Event::CData(e) => {
                if let Some((_, _, text)) = &mut field {
                    text.push_str(&e.decode().map_err(|_| ())?);
                }
            }
            Event::GeneralRef(e) => {
                if let Some((_, _, text)) = &mut field {
                    text.push('&');
                    text.push_str(&e.decode().map_err(|_| ())?);
                    text.push(';');
                }
            }
            Event::End(_) => {
                if field
                    .as_ref()
                    .is_some_and(|(_, depth, _)| *depth == path.len())
                {
                    let (name, _, text) = field.take().unwrap();
                    if let Some(i) = &mut item {
                        match name.as_str() {
                            "title" => i.title = plain(&text),
                            "link" if i.link.is_empty() => {
                                i.link = safe_link(
                                    &quick_xml::escape::unescape(text.trim()).unwrap_or_default(),
                                )
                            }
                            "guid" | "id" => i.guid = plain(&text),
                            "description" | "summary" => i.summary = plain(&text),
                            "pubDate" | "updated" | "published" => i.date = text.trim().into(),
                            _ => {}
                        }
                    } else {
                        feed.title = plain(&text);
                    }
                }
                if (path == ["rss", "channel", "item"] || path == ["feed", "entry"])
                    && item.is_some()
                {
                    let mut i = item.take().unwrap();
                    if i.guid.is_empty() {
                        i.guid = if !i.link.is_empty() {
                            i.link.clone()
                        } else {
                            format!("text:{}|{}|{}", i.title, i.date, i.summary)
                        };
                    }
                    i.published_ms = chrono::DateTime::parse_from_rfc2822(&i.date)
                        .or_else(|_| chrono::DateTime::parse_from_rfc3339(&i.date))
                        .map(|d| d.timestamp_millis())
                        .unwrap_or(now);
                    if i.title.is_empty() {
                        i.title = "Untitled item".into();
                    }
                    feed.items.push(i);
                    if feed.items.len() == 500 {
                        return Ok(feed);
                    }
                }
                path.pop().ok_or(())?;
            }
            Event::DocType(_) => return Err(()), // No DTDs or custom entities.
            Event::Eof => break,
            _ => {}
        }
    }
    if !recognized || !path.is_empty() {
        return Err(());
    }
    Ok(feed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_rss2_items() {
        let f = parse_feed(r#"<rss version="2.0"><channel><title>News &amp; more</title><item><guid>one</guid><title>Hello</title><link>https://example.com/one</link><description>Hi</description><pubDate>Tue, 10 Nov 2026 12:00:00 +0000</pubDate></item></channel></rss>"#, 42).unwrap();
        assert_eq!(f.title, "News & more");
        let i = &f.items[0];
        assert_eq!(i.guid, "one");
        assert_eq!(i.title, "Hello");
        assert_eq!(i.summary, "Hi");
        assert_eq!(i.published_ms, 1794312000000);
    }
    #[test]
    fn parses_atom_entries() {
        let f = parse_feed(r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom</title><entry><id>urn:one</id><title>Hello</title><link rel="self" href="https://example.com/self"/><link href="https://example.com/one?a=1&amp;b=2"/><summary type="xhtml"><div><b>Good</b> news</div></summary><updated>2026-11-10T12:00:00Z</updated></entry></feed>"#, 42).unwrap();
        assert_eq!(f.items[0].link, "https://example.com/one?a=1&b=2");
        assert_eq!(f.items[0].guid, "urn:one");
        assert_eq!(f.items[0].summary, "Good news");
        assert_eq!(f.items[0].published_ms, 1794312000000);
    }
    #[test]
    fn strips_html_from_summaries() {
        let f = parse_feed(r#"<rss><channel><item><description><![CDATA[<script>alert(1)</script><p>Hello &amp; <b>world</b></p>]]></description><link>javascript:alert(1)</link></item></channel></rss>"#, 42).unwrap();
        assert_eq!(
            plain("&lt;p&gt;Encoded &amp;amp; safe&lt;/p&gt;"),
            "Encoded & safe"
        );
        assert_eq!(f.items[0].summary, "alert(1) Hello & world");
        assert_eq!(f.items[0].link, "");
        assert_eq!(f.items[0].published_ms, 42);
    }
    #[test]
    fn ignores_entries_beyond_cap() {
        let body = format!(
            "<rss><channel>{}</channel></rss>",
            "<item><title>Hi</title></item>".repeat(510)
        );
        assert_eq!(parse_feed(&body, 42).unwrap().items.len(), 500);
    }
    #[test]
    fn missing_guid_falls_back_to_link() {
        let f = parse_feed(
            "<rss><channel><item><link>https://example.com/a</link></item></channel></rss>",
            42,
        )
        .unwrap();
        assert_eq!(f.items[0].guid, f.items[0].link);
    }
    #[test]
    fn rejects_invalid_xml_and_unsafe_urls() {
        for xml in ["", "<html/>", "<rss><channel>", "<!DOCTYPE rss><rss/>"] {
            assert!(parse_feed(xml, 42).is_err(), "{xml}");
        }
        for u in [
            "http://example.com",
            "https://a:b@example.com",
            "https://example.com:444",
            "https://127.0.0.1",
            "https://example.com/#x",
        ] {
            assert!(feed_url(u).is_err());
        }
        for u in [
            "javascript:alert(1)",
            "data:text/html,test",
            "//example.com",
            "https://a:b@example.com",
            "https://example.com/\nattack",
        ] {
            assert!(safe_link(u).is_empty());
        }
    }
}
