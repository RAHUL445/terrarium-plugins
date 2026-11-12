# Plugin roadmap

Open plugin work before a stable or binary release. The host's
[roadmap](https://github.com/RAHUL445/terrarium/blob/main/docs/roadmap.md) is
the canonical product security and release backlog.

## Built in the unreleased source

- To-do 0.5.0: indexed search and references, linked notes, custom views and
  card moves, bulk actions, CSV import/export, completion events and History.
- Notes 0.3.3: indexed search and references, mentions and backlinks, custom
  views, bulk changes, Android sharing, Markdown import/export and History.
- Bookmarks 0.2.3: indexed search and references, custom views, bulk actions
  and Android sharing.
- Voice 0.2.3: indexed captures and references; desktop capture wording.
- Habits 0.3.0: search, references and check-in events.
- Reports 0.3.4: saved report search and references.
- Feeds 0.1.0: RSS/Atom reading through approved host HTTPS access.
- Deterministic unsigned registry indexes from explicit reviewed artifacts.
  Signing and production registry activation remain separate host steps.

See [CHANGELOG.md](../CHANGELOG.md). Upgrade all paired hosts before using
packages with new manifest declarations.

## Remaining release work

- [ ] Generate per-artifact dependency/license inventory and required notices
  before distributing plugin binaries or model files.
- [ ] Complete strict optional-field typing and fuzz operation/template payloads.
- [ ] Prove audio capture/retry resource limits and latency on representative
  Android devices; review every error/callback/notification preview path.
- [ ] Add actual history/audio erasure through host-owned capabilities.
  History visibility periods are built; they do not compact logs or erase
  audio. Clearing an active audio field is not secure erasure.
- [ ] Retest least-privilege manifests whenever operations/interfaces change;
  preserve rollback and real WASM hostile-input/capability-escalation tests.
- [ ] Automate paired host/plugin release provenance and compatibility matrix;
  matching WIT/API strings alone are insufficient.

Host-owned deferred work: dependency findings/backport review, full device
revocation, durable key rotation, hard resource quotas, signed native artifacts,
backup recovery, platform/store validation and accessibility. Detailed designs
are in the host security documents; none is silently marked done here.
