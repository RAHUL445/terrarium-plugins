## Summary

<!-- What does this change and why? Link the issue if there is one. -->

## Checks

- [ ] `cargo fmt` and `cargo build --release --target wasm32-unknown-unknown` for changed plugins
- [ ] Plugin unit tests and `packages/notes/tests` SQL tests
- [ ] `TERRA_HOST_SOURCE=../terrarium node --test tests/*.test.cjs`
- [ ] Manifest permissions are still minimal
- [ ] CHANGELOG updated where relevant
