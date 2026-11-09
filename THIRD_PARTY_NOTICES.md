# Third-party source notices

Original plugins and the Terrarium guest SDK use Apache-2.0. The pinned
`vendor/terrarium-sdk/` snapshot is original host-project code, not third-party
Loro code. Its embedded LICENSE/NOTICE files and `sdk.lock.json` identify the
licensed SDK snapshot. SDK/WIT provenance is retained in SOURCE metadata.

Rust dependencies and any separately installed speech or AI models retain their
own licenses. Preserve original notices. Package Cargo.lock files identify exact
dependencies but are not complete binary redistribution/license reports.

Before distributing plugin binaries or model files, generate the matching
license inventory and include required licenses/notices. See `docs/roadmap.md`.
