# Contributing to Autoteur

Thanks for helping build the director's chair.

## Ground rules

- **File formats are the product.** Changes to any schema under `docs/proposals/` need a proposal update first — the formats are load-bearing for both the GUI and every coding agent working in user projects.
- **Authored vs derived:** never add a field to an authored file that the app could compute and disagree with (take counts, queue state, costs). See proposal 0001.
- **No `.unwrap()` outside tests.** Errors are `thiserror` in library crates, `anyhow` at binaries' edges.
- **Surgical writes only.** All programmatic TOML edits go through `toml_edit` document editing — never deserialize-and-reserialize; comments and unknown keys must round-trip byte-for-byte.

## Workflow

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same three on Windows and Linux. PRs need green CI and a test for any behavior change.

## License

By contributing you agree your contributions are dual-licensed under MIT and Apache-2.0.
