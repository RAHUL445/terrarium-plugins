# Plugin roadmap

Open plugin work before a stable or binary release. The host's
[roadmap](https://github.com/RAHUL445/terrarium/blob/main/docs/roadmap.md) is
the canonical product security and release backlog.

- [ ] Generate per-artifact dependency/license inventory and required notices
  before distributing plugin binaries or model files.
- [ ] Complete strict optional-field typing and fuzz operation/template payloads.
- [ ] Prove audio capture/retry resource limits and latency on representative
  Android devices; review every error/callback/notification preview path.
- [ ] Add retention/history erasure controls through host-owned capabilities;
  clearing an active audio field is not secure erasure.
- [ ] Retest least-privilege manifests whenever operations/interfaces change;
  preserve rollback and real WASM hostile-input/capability-escalation tests.
- [ ] Automate paired host/plugin release provenance and compatibility matrix;
  matching WIT/API strings alone are insufficient.

Host-owned deferred work: dependency findings/backport review, full device
revocation, durable key rotation, hard resource quotas, signed native artifacts,
backup recovery, platform/store validation and accessibility. Detailed designs
are in the host security documents; none is silently marked done here.
