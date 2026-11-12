# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed in UI review round 2

- Unsaved Bookmarks and other quick-entry drafts can be kept while navigating between apps (host fix).
- Bookmarks updates the All count even when no project has been assigned.
- Feeds shows locale dates without times, distinguishes unread articles, omits repeated grouped sources, and uses article links, explicit read actions, one Refresh button and consistent filters.
- Bookmarks merges its count into the filters and shows project/tag save hints only when selected.
- Bookmarks and Reports display local times consistently.
- Reports and Voice show one empty message per section, and empty sleep charts explain that nothing is logged.
- To-do uses compact board actions, accurate filter help and an Unsorted matrix group.
- Today starts with its No date group collapsed and remembers expansion in the saved view.
- Habits provides titled choices for automation check-ins and the Gym example.
- Habits keeps completed titles readable and shows percentage units in insights.
- The Home habit widget explains how to add habits instead of showing an empty grid.
- Task search includes due dates, tags and a note excerpt to distinguish similar titles.
- New-task reminders stay blank until a due date is set.
- Notes consistently uses New note, and Voice uses New capture.

### Added

- **To-do 0.5.0:** indexed task search, item references and linked note
  context; viewable task data with board and matrix moves; bulk completion,
  updates, tags, trash and restore; CSV export and task import; item History.
  Real completions emit `task_done` for approved automations.
- **Notes 0.3.3:** indexed search, item chips, @mentions and Linked from;
  viewable note data; bulk project changes, trash and restore; Android text
  sharing; Markdown export and folder import; item History.
- **Bookmarks 0.2.3:** indexed search and item references, viewable bookmark
  data, bulk pin/unpin, trash and restore, and Android text/link sharing.
- **Voice 0.2.3:** indexed capture search and item references.
- **Habits 0.3.0:** search and item references. Effective check-in changes
  emit `checked_in` without notes; repeating the same day/status is harmless.
- **Reports 0.3.4:** search and item references for saved reports.
- **Feeds 0.1.0:** public HTTPS RSS/Atom subscriptions, conditional refresh,
  text summaries, read/unread controls and an unread widget. Keeps the newest
  200 items per feed plus older unread items, and refreshes up to 20 feeds
  per run. Hourly schedules default to desktop. Requires an HTTP-capable host
  and explicit **Reach any website** approval on each device, including for
  sites not added as feeds. Runs only while the app is open (foreground on
  Android); feed content never automatically loads remote images.
- Deterministic unsigned catalog generation for reviewed plugin artifacts;
  registry signing is a separate host CLI step.
- First public source preview of the original six plugins, release bundle
  builder with SDK verification and digest pinning, and a vendored SDK for
  `terrarium:plugin@0.1.0`.
- Command descriptions in every plugin manifest for command-line clients
  and agents: typed arguments, effects, confirmation questions, examples,
  item destinations and hidden internal commands.

### Changed

- **Voice 0.2.3 and Habits 0.3.0:** desktop instructions follow the input
  device. **Habits 0.3.0:** new reminders default to the device in use.

### Fixed

- **To-do 0.5.0:** completion says **Task done** and offers the optional note
  action in a toast; saving does not interrupt you with a note prompt.

Upgrade Terrarium on every paired device before installing these packages.
New manifest declarations need matching host support; event-enabled packages
also require reviewing automations on the chosen execution device.
