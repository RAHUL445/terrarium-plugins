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

Each plugin is a Rust crate compiled to a WebAssembly component, plus a
manifest (storage schema, queries, permissions, schedules) and declarative UI
templates. The Terrarium host runs them in a sandbox on macOS and Android and
syncs their data between devices.

> **Status:** experimental source preview, not a production release.

## Recommended plugins

| Plugin | What it does |
| --- | --- |
| [To-do](packages/todo) | Tasks with due dates, reminders, priorities, repeats and steps; list, board and matrix layouts |
| [Notes](packages/notes) | Markdown notes in projects with checklists and autosave; edits on different devices merge |
| [Habits](packages/habits) | Scheduled habits with check-ins, streaks, reminders and sleep logs |
| [Bookmarks](packages/bookmarks) | Links with projects, tags and notes; pin the ones you use most |
| [Voice](packages/voice) | Record or type quick captures, transcribed locally, then send them to To-do or Notes |
| [Reports](packages/reports) | AI-written reviews of your tasks, habits and notes, on demand or on a schedule |

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

The script verifies the SDK snapshot, builds and runtime-checks all six
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
