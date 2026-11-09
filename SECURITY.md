# Security policy

Terrarium plugins are an experimental source preview. No version is a stable
or security-certified release. Fixes are best effort with no guaranteed
response time.

## Reporting a vulnerability

Please report privately through GitHub:
**Security → Advisories → Report a vulnerability** on this repository.
Do not open a public issue for security problems.

Include the affected commit, plugin and platform, the impact, and a minimal
reproduction using synthetic data. Never include real credentials, recovery
phrases, personal records or logs containing them.

## Scope

In scope: plugin input validation, manifest permissions and queries, data
handling in the six first-party plugins, and the release bundle builder.
The sandbox, sync, signing and native services are enforced by the host; see
the [host security policy](https://github.com/RAHUL445/terrarium/blob/main/SECURITY.md)
and its threat model. First-party plugin checks cannot make a malicious plugin
safe; the host must enforce every boundary.

## Known limits

- Clearing an audio or text field does not erase earlier synced or backed-up
  copies; retention controls are not built yet.
- Operation and template payloads are not fuzzed yet.
- Plugin binaries are not distributed with a full license inventory.

See the [input review](docs/input-review.md) and [roadmap](docs/roadmap.md).
