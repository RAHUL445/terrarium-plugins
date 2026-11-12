<p align="center">
  <img src="branding/terrarium-logo.svg" alt="Terrarium logo: six tiles inside a rounded frame" width="128">
</p>

<h1 align="center">Terrarium plugins</h1>

<p align="center">
  Recommended plugins for <a href="https://github.com/RAHUL445/terrarium">Terrarium</a>, the extensible, local-first app host.
</p>

<p align="center">
  <a href="https://github.com/RAHUL445/terrarium-plugins/actions/workflows/checks.yml"><img src="https://github.com/RAHUL445/terrarium-plugins/actions/workflows/checks.yml/badge.svg" alt="Checks"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License: Apache-2.0"></a>
  <img src="https://img.shields.io/badge/status-experimental-orange.svg" alt="Status: experimental">
</p>

Terrarium is a host; these plugins give it its features. They use only the
public SDK and WIT contract, so they are also working examples for writing your
own.

These plugins are Rust crates compiled to WebAssembly components, each with a
manifest (storage schema, queries, permissions, schedules) and declarative UI
templates. The Terrarium host runs them in a sandbox on macOS and Android and
syncs their data between devices.

> **Status:** experimental source preview, not a production release.

## Recommended plugins

| Plugin | What it does |
| --- | --- |
| [To-do](packages/todo) | Tasks, repeats, reminders and steps; board/matrix moves, bulk tags, CSV import/export, links and History |
| [Notes](packages/notes) | Markdown notes with merging, mentions, backlinks and attachments; custom views, bulk changes, Android sharing, import/export and History |
| [Habits](packages/habits) | Scheduled habits, check-ins, streaks, reminders and sleep logs; search, references and check-in events |
| [Bookmarks](packages/bookmarks) | Links with projects, tags and notes; indexed search, references, views, bulk pin/trash and Android sharing |
| [Voice](packages/voice) | Record or type quick captures, transcribe locally, search and link them, then send them to To-do or Notes |
| [Feeds](packages/feeds) | HTTPS RSS/Atom reader with text summaries, read/unread controls, hourly desktop refresh and an unread widget; asks to reach any website |
| [Reports](packages/reports) | AI-written reviews on demand or on a schedule; search and link saved reports |

Search and item links use host-owned indexes and reference resolution; plugins
keep their own data. To-do completions and Habits check-ins can trigger approved
[automations](https://github.com/RAHUL445/terrarium/blob/main/docs/guide/automations.md).
Upgrade all paired hosts before installing the new packages, then review their
permissions on each device. Feeds needs explicit website access; accounts and
browser sign-in are host features, not built-in Feeds requirements.

To publish catalog metadata, `scripts/build-index.py` builds deterministic
unsigned indexes from reviewed package artifacts. Sign them separately with
`terra sign-index`; see the host's
[registry design](https://github.com/RAHUL445/terrarium/blob/main/docs/design/registry.md).
Production catalog activation is still gated.

## Getting started

Prerequisites: Rust stable with `wasm32-unknown-unknown` (pinned in
`rust-toolchain.toml`) and Python 3. Building runnable packages also needs a
[Terrarium](https://github.com/RAHUL445/terrarium) checkout for the `terra` CLI.

Clone both repositories side by side:

```sh
git clone https://github.com/RAHUL445/terrarium.git
git clone https://github.com/RAHUL445/terrarium-plugins.git
cd terrarium && cargo build --locked -p terra && cd ..
```

Build every plugin and try them in a browser:

```sh
cd terrarium-plugins
for p in packages/*/; do ../terrarium/target/debug/terra build "$p"; done
../terrarium/target/debug/terra preview packages/* --port 8767 --data /tmp/terrarium-preview
```

Open <http://localhost:8767>. Use `terra dev packages/<id>` to rebuild a plugin
on every change.

To use the plugins in the desktop or Android app, follow the host's
[install guides](https://github.com/RAHUL445/terrarium/tree/main/docs#user-guide).

## Checks

These run without a host checkout:

```sh
python3 -c "import runpy; runpy.run_path('scripts/build-release.py')['verify_sdk']()"
for m in packages/*/Cargo.toml; do
  cargo build --locked --release --target wasm32-unknown-unknown --manifest-path "$m"
done
python3 -m unittest discover -s packages/notes/tests -p '*_query.py'
```

UI integration tests need the host UI:

```sh
TERRA_HOST_SOURCE=../terrarium node --test tests/*.test.cjs
```

The host's pinned integration workflow runs the full Rust integration suite
and app build against an exact plugins commit.

## Release bundles

The host app embeds a digest-pinned bundle built here:

```sh
python3 scripts/build-release.py \
  --terra ../terrarium/target/debug/terra \
  --output releases/dev-001
```

The script verifies the SDK snapshot, builds and runtime-checks all seven
plugins, and exports only manifests, referenced UI templates and
`dist/plugin.wasm`. `bundle.json` pins versions, digests, SDK provenance and
the plugins commit; builds from a dirty checkout are marked
`source_dirty: true`. Existing output directories are never overwritten. The
host then imports the directory with its `scripts/import-plugins.py`.

## SDK

`vendor/terrarium-sdk/` is a pinned snapshot of the host's guest SDK and WIT
contract (`terrarium:plugin@0.1.0`); `sdk.lock.json` records the host commit and
file digests. Never edit it by hand. To update it, run the host's
`scripts/export-sdk.py`, replace `vendor/terrarium-sdk/` and `sdk.lock.json`
together, rebuild every plugin and run the integration checks.

Writing a new plugin: see the host's
[plugin authoring guide](https://github.com/RAHUL445/terrarium/blob/main/docs/develop/plugin-authoring.md).

## Documentation

- [Input and capability review](docs/input-review.md)
- [Privacy and data handling](PRIVACY.md)
- [Security policy](SECURITY.md)
- [Roadmap](docs/roadmap.md)
- [Releasing](docs/releasing.md)

## Contributing

Issues and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Original code and the vendored SDK are licensed under [Apache-2.0](LICENSE).
Third-party code keeps its own license; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
