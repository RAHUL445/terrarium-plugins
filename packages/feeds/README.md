# Feeds

A reference network plugin: add a public HTTPS RSS 2.0 or Atom feed, read its
items, and mark them read. The page groups items by feed; the widget shows
the latest ten unread items. Refresh runs hourly on desktop while Terrarium
is open, using ETag and Last-Modified validators when available.

## Network access

This plugin declares `http` and `hosts: ["*"]`. It can send data it can read
to any public website, including sites you have not added. Installing it
requires the host's explicit any-website consent. Feed URLs alone grant no
permission. The host blocks local/private destinations and non-HTTPS feeds;
article links may open HTTP or HTTPS in your browser. Feed text is plain text,
and other link schemes are discarded. No credentials are required.

Upgrade every device to a host supporting the HTTP interface before enabling
this plugin in a synced space. Older hosts cannot load it. This sample is
not part of the bundled plugin set.

The parser reads at most 500 entries per reply, deduplicates by feed and guid
(or article link when no guid is supplied), and prunes older read entries
outside the newest 200. Older unread entries are retained. A feed without an
entry id or usable link uses its title, date and summary as a stable fallback.
Read-item tombstones prevent a later refresh from recreating pruned entries.
A failed reply leaves existing items in place and records an error on the feed.

```sh
cargo test --locked --manifest-path packages/feeds/Cargo.toml
# From the host checkout, with CARGO_TARGET_DIR set as appropriate:
cargo run --locked -p terra -- build ../terrarium-plugins/packages/feeds
cargo run --locked -p terra -- preview ../terrarium-plugins/packages/feeds --port 8767 --data /tmp/terrarium-feeds-preview
```
