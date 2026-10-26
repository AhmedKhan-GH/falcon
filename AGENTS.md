# Notes for coding agents

- The workspace is `crates/falcon-library` (library format, import, search), `crates/falcon-server` (HTTP API), `crates/falcon-app` (egui desktop app) and the `falcon` CLI in `src/`.
- Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` before committing. Both must pass.
- Library writes must stay byte-compatible with existing libraries. `crates/falcon-library/tests/fixture.rs` checks round trips against a library in `fixtures/`, which is gitignored and supplied locally; without it, those tests skip.
- The app draws everything with egui painters. Colours and sizes live in `crates/falcon-app/src/theme.rs`, and layout logic lives in `crates/falcon-app/src/logic.rs`, which has unit tests.
- `docs/` is an mdBook describing each part. When you change behaviour, update the matching chapter.
