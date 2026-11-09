# Contributing to Terrarium plugins

Thanks for your interest. Small, focused changes are the easiest to review.
For anything larger than a bug fix, open an issue first.

Security problems go through private reporting; see [SECURITY.md](SECURITY.md).
Changes to the runtime, sync, UI shell or SDK belong in
[terrarium](https://github.com/RAHUL445/terrarium).

## Setup

Follow [Getting started](README.md#getting-started) to build the plugins and
preview them against a host checkout. The host's
[plugin authoring guide](https://github.com/RAHUL445/terrarium/blob/main/docs/plugin-authoring.md)
covers manifests, storage, queries and templates.

## Guidelines

- Validate every operation argument (types, lengths, ranges) before storing it.
- Request the fewest permissions and interfaces a plugin needs.
- Storage migrations are additive only; never rename or drop synced columns.
- Templates stay declarative: no scripts or raw HTML.
- Never edit `vendor/terrarium-sdk/` by hand; refresh it from the host.
- Add tests with synthetic data, and run the [checks](README.md#checks).
- Use [Conventional Commits](https://www.conventionalcommits.org/), for example
  `fix(todo): reject invalid reminder times`.

## License

By contributing you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE).
