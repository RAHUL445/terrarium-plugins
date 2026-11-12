# Releasing

Plugin releases are digest-pinned bundles that the host imports. No plugin
binaries are distributed separately yet.

1. Make sure `main` is clean and green.
2. Build `terra` from the host commit recorded in `sdk.lock.json`, then build a
   bundle from this clean checkout:

   ```sh
   python3 scripts/build-release.py --terra /path/to/terrarium/target/debug/terra \
     --output releases/<new-name>
   ```

   The output directory must not exist. Bundles with `source_dirty: true` are
   development artifacts only.
3. In the host, import it with `scripts/import-plugins.py <dir> --update-lock`,
   review the lock diff and commit it.
4. Run the host's `Pinned plugin integration` workflow with this repository and
   the full commit SHA recorded in the host's `plugins.lock.json`.
5. Update [CHANGELOG.md](../CHANGELOG.md) and tag the release.

The plugin API is `terrarium:plugin@0.1.0`; a matching API string alone does not
prove compatibility. Do not distribute WASM or model binaries until the
artifact license, provenance and platform gates in the [roadmap](roadmap.md)
are reviewed.

When refreshing the SDK for declared events, upgrade every host/device before
publishing event-enabled packages. The API stays `terrarium:plugin@0.1.0`, but
older hosts reject `events` declarations/imports. Existing components continue
to load on the new host. Never strip declarations for an older host.

A host checkout that cannot yet be committed may export with
`python3 scripts/export-sdk.py --output <new-dir> --development` for local
validation. Such snapshots carry `source_dirty: true` and must not be released.
After committing the host, re-export without that option and replace the SDK
and lock together before committing the plugins snapshot.
