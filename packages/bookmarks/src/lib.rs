//! Bookmarks: links with a title, project, tags, note and a pin. Listing,
//! filters and search are manifest queries; this component parses, validates
//! and writes.

use serde_json::{json, Map, Value};
use terrarium_guest::{clock, storage, Args, Error, Plugin, Result};

struct Bookmarks;

const TABLE: &str = "bookmarks";

impl Plugin for Bookmarks {
    fn handle(operation: &str, args: Args) -> Result<Value> {
        match operation {
            "create" => create(&args),
            "update" => update(&args),
            "pin" => pin(&args),
            "visit" => {
                let id = args.str("id")?;
                storage::increment(TABLE, id, "visits", json!(1))?;
                let mut values = Map::new();
                values.insert("visited_at".into(), json!(clock::now_ms() as i64));
                storage::update(TABLE, id, &values)?;
                Ok(Value::Null)
            }
            "delete" => {
                storage::delete(TABLE, args.str("id")?)?;
                Ok(Value::Null)
            }
            "restore" => {
                storage::restore(TABLE, args.str("id")?)?;
                Ok(Value::Null)
            }
            _ => Err(Error::unknown_operation(operation)),
        }
    }
}

/// Saves a link. Takes `url` (with optional `title`), or `text` as typed in
/// the quick-save field: the first web address in it is the link, `#words`
/// are tags and the remaining words the title. `tags`, `project`, `note` and
/// `pinned` are optional. Saving an address that is already saved returns
/// that bookmark instead of a copy.
fn create(args: &Args) -> Result<Value> {
    let (url, typed_title, mut tags) = match args.opt_str("text") {
        Some(text) => parse_text(text)?,
        None => (args.str("url")?.to_string(), None, Vec::new()),
    };
    let url = clean_url(&url)?;
    tags.extend(split_tags(args.opt_str("tags").unwrap_or("")));
    let tags = join_tags(tags)?;

    let existing = storage::query(
        "SELECT id FROM bookmarks WHERE _deleted IS NULL AND url = ?1 LIMIT 1",
        &[json!(url)],
    )?;
    if let Some(id) = existing.first().and_then(|r| r.get("id")).cloned() {
        return Ok(json!({ "id": id, "existing": true, "message": "Already saved" }));
    }

    let title = match args
        .opt_str("title")
        .or(typed_title.as_deref())
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(title) => limit(title, 300, "Title")?,
        None => host_of(&url),
    };
    let mut values = Map::new();
    values.insert("url".into(), json!(url));
    values.insert("title".into(), json!(title));
    values.insert("created_at".into(), json!(clock::now_ms() as i64));
    if !tags.is_empty() {
        values.insert("tags".into(), json!(tags));
    }
    if let Some(project) = clean_project(args.opt_str("project").unwrap_or(""))? {
        values.insert("project".into(), project);
    }
    if let Some(note) = args
        .opt_str("note")
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        values.insert("note".into(), json!(limit(note, 2000, "Note")?));
    }
    if flag(args, "pinned") == Some(true) {
        values.insert("pinned".into(), json!(true));
    }
    let id = storage::insert(TABLE, &values)?;
    Ok(json!({ "id": id, "existing": false, "message": "Saved" }))
}

fn update(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let mut values = Map::new();
    if let Some(url) = args.opt_str("url") {
        values.insert("url".into(), json!(clean_url(url)?));
    }
    if let Some(title) = args.opt_str("title") {
        let title = title.trim();
        if title.is_empty() {
            let url = match args.opt_str("url") {
                Some(url) => clean_url(url)?,
                None => current_url(id)?,
            };
            values.insert("title".into(), json!(host_of(&url)));
        } else {
            values.insert("title".into(), json!(limit(title, 300, "Title")?));
        }
    }
    if let Some(tags) = args.opt_str("tags") {
        let tags = join_tags(split_tags(tags))?;
        values.insert(
            "tags".into(),
            if tags.is_empty() {
                Value::Null
            } else {
                json!(tags)
            },
        );
    }
    if let Some(project) = args.opt_str("project") {
        values.insert(
            "project".into(),
            clean_project(project)?.unwrap_or(Value::Null),
        );
    }
    if let Some(note) = args.opt_str("note") {
        let note = note.trim();
        values.insert(
            "note".into(),
            if note.is_empty() {
                Value::Null
            } else {
                json!(limit(note, 2000, "Note")?)
            },
        );
    }
    if let Some(pinned) = flag(args, "pinned") {
        values.insert("pinned".into(), json!(pinned));
    }
    if values.is_empty() {
        return Err(Error::invalid("nothing to update"));
    }
    storage::update(TABLE, id, &values)?;
    Ok(Value::Null)
}

/// Pins or unpins a bookmark; without `pinned` it flips the current state.
fn pin(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let pinned = match flag(args, "pinned") {
        Some(pinned) => pinned,
        None => {
            let rows = storage::query(
                "SELECT pinned FROM bookmarks WHERE id = ?1 AND _deleted IS NULL",
                &[json!(id)],
            )?;
            let row = rows
                .first()
                .ok_or_else(|| Error::new("not_found", "That bookmark is gone"))?;
            !row.get("pinned")
                .is_some_and(|v| v.as_bool() == Some(true) || v.as_i64() == Some(1))
        }
    };
    let mut values = Map::new();
    values.insert("pinned".into(), json!(pinned));
    storage::update(TABLE, id, &values)?;
    Ok(json!({ "pinned": pinned, "message": if pinned { "Pinned" } else { "Unpinned" } }))
}

fn current_url(id: &str) -> Result<String> {
    let rows = storage::query("SELECT url FROM bookmarks WHERE id = ?1", &[json!(id)])?;
    rows.first()
        .and_then(|r| r.get("url"))
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| Error::new("not_found", "That bookmark is gone"))
}

/// A boolean argument; forms send toggles as "true" / "false".
fn flag(args: &Args, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| match v {
        Value::String(s) if s == "true" => Some(true),
        Value::String(s) if s == "false" => Some(false),
        v => v.as_bool(),
    })
}

/// Splits quick-save text into (address, title, tags).
fn parse_text(text: &str) -> Result<(String, Option<String>, Vec<String>)> {
    let mut url = None;
    let mut title = Vec::new();
    let mut tags = Vec::new();
    for word in text.split_whitespace() {
        if let Some(tag) = word.strip_prefix('#').filter(|t| !t.is_empty()) {
            tags.push(tag.to_lowercase());
        } else if url.is_none() && looks_like_url(word) {
            url = Some(word.trim_end_matches([',', ')', '>']).to_string());
        } else {
            title.push(word);
        }
    }
    let url = url.ok_or_else(|| Error::invalid("Paste a web address like https://example.com"))?;
    let title = title.join(" ");
    let title = title.trim_matches(|c: char| c == '-' || c == '–' || c == '|' || c.is_whitespace());
    Ok((url, (!title.is_empty()).then(|| title.to_string()), tags))
}

fn looks_like_url(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return true;
    }
    let host = lower.split(['/', '?', '#']).next().unwrap_or("");
    let name = host.split(':').next().unwrap_or("");
    !lower.contains("://")
        && (name == "localhost"
            || (name.contains('.')
                && !name.ends_with('.')
                && name.rsplit('.').next().is_some_and(|tld| {
                    tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
                })))
}

/// Accepts http(s) URLs only; adds https:// when no scheme is given and
/// drops tracking parameters (utm_*, fbclid, gclid and the like).
fn clean_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    let lower_raw = raw.to_ascii_lowercase();
    // Any other scheme (javascript:, data:, file:, ftp:) is refused outright.
    let has_scheme = lower_raw.contains("://")
        || lower_raw.split(['/', '?', '#']).next().is_some_and(|head| {
            head.split_once(':').is_some_and(|(_, port)| {
                !port.chars().all(|c| c.is_ascii_digit()) || port.is_empty()
            })
        });
    let url = if has_scheme {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    let host = rest.map(|r| r.split(['/', '?', '#']).next().unwrap_or(""));
    let host_ok = |h: &str| {
        let (name, port) = h.split_once(':').unwrap_or((h, ""));
        let port_ok = port.is_empty() || port.parse::<u16>().is_ok_and(|p| p > 0);
        let name_ok = !name.is_empty()
            && !name.starts_with(['.', '-'])
            && !name.ends_with(['.', '-'])
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            && (name.contains('.') || name == "localhost");
        port_ok && name_ok && !h.ends_with(':')
    };
    let valid = host.is_some_and(host_ok)
        && url.len() <= 2048
        && !url.chars().any(|c| c.is_control() || c == ' ');
    if !valid {
        return Err(Error::invalid(
            "Enter a web address like https://example.com",
        ));
    }
    Ok(strip_tracking(&url))
}

fn strip_tracking(url: &str) -> String {
    let (base, fragment) = match url.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (url, None),
    };
    let Some((path, query)) = base.split_once('?') else {
        return url.to_string();
    };
    let tracking = |key: &str| {
        let key = key.to_ascii_lowercase();
        key.starts_with("utm_")
            || [
                "fbclid", "gclid", "dclid", "msclkid", "mc_cid", "mc_eid", "igshid",
            ]
            .contains(&key.as_str())
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| !p.is_empty() && !tracking(p.split('=').next().unwrap_or("")))
        .collect();
    let mut out = path.to_string();
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .trim_start_matches("www.")
        .to_string()
}

fn limit(text: &str, max: usize, label: &str) -> Result<String> {
    if text.chars().count() > max {
        return Err(Error::invalid(format!(
            "{label} must be {max} characters or fewer"
        )));
    }
    Ok(text.to_string())
}

fn clean_project(project: &str) -> Result<Option<Value>> {
    let project = project.trim();
    if project.is_empty() {
        return Ok(None);
    }
    Ok(Some(json!(limit(project, 60, "Project")?)))
}

/// Tags from "rust, docs" or "#rust #docs": lowercased, without '#'.
fn split_tags(tags: &str) -> Vec<String> {
    tags.split([',', '#'])
        .map(|t| {
            t.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

/// Joins tags as "rust, docs", dropping repeats.
fn join_tags(tags: Vec<String>) -> Result<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        if tag.chars().count() > 40 {
            return Err(Error::invalid("Tags must be 40 characters or fewer"));
        }
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    if out.len() > 20 {
        return Err(Error::invalid("Use 20 tags or fewer"));
    }
    Ok(out.join(", "))
}

terrarium_guest::plugin!(Bookmarks);
