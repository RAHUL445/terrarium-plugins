# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- To-do 0.5.0 emits `task_done` on real task completion; Habits 0.3.0 emits
  `checked_in` on effective check-in changes, without notes. Repeating the same
  day/status check-in is harmless. Upgrade Terrarium on every device before
  installing or syncing these event-enabled packages, then review automations
  on the chosen execution device.
- Feeds (`feeds` 0.1.0): public HTTPS RSS/Atom subscriptions, conditional
  refresh, text summaries, read/unread controls and an unread widget. Keeps
  the newest 200 items per feed plus older unread items, and refreshes up to
  20 feeds per run; hourly schedules default to desktop. Requires an HTTP-capable host on every paired device
  and explicit “Reach any website” approval on each device. This permission
  allows sending readable data to public websites, including sites not added
  as feeds. Requests run only while the app is open (foreground on Android);
  feed content never automatically loads remote images.

- First public source preview of the six first-party plugins: To-do, Notes,
  Habits, Bookmarks, Voice and Reports.
- Release bundle builder with SDK verification and digest pinning.
- Vendored guest SDK snapshot for `terrarium:plugin@0.1.0`.
- Command descriptions in every plugin manifest for the `terrarium` command
  line and agents: typed arguments, effects, confirmation questions,
  examples, the page items open on, and internal commands hidden with
  `"cli": false`. These manifests need a host with the command registry.
