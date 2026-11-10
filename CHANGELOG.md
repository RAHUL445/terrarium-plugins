# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- First public source preview of the six first-party plugins: To-do, Notes,
  Habits, Bookmarks, Voice and Reports.
- Release bundle builder with SDK verification and digest pinning.
- Vendored guest SDK snapshot for `terrarium:plugin@0.1.0`.
- Command descriptions in every plugin manifest for the `terrarium` command
  line and agents: typed arguments, effects, confirmation questions,
  examples, the page items open on, and internal commands hidden with
  `"cli": false`. These manifests need a host with the command registry.
