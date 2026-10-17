# Contributing

Start with architecture and compatibility. Keep claims tied to executable behavior and repeatable evidence.

- Write source comments, documentation, issues and pull requests in English.
- Never commit the FSX installation, SDK binaries, extracted geometry, textures or commercial add-ons.
- Use small original fixtures. Test truncation, invalid references and resource limits as well as successful decoding.
- Keep subsystem code in its `crates/fsx-*` package; the headless tool belongs in `tools/fsx-check`, launcher markup in `launcher/`, and shaders in `assets/shaders/`.
- Run `cargo test --workspace --all-features --locked`, `cargo fmt --all --check`, `node scripts/check-architecture.mjs` and the fsx-app viewer build when changing rendering interfaces.
- Regenerate the site with `node scripts/build-site.mjs` after editing `docs/`.
- When visible engine behavior changes, refresh the website's selected screenshots with real openFSX captures and update their captions and capture record. Keep source game resources and intermediate captures excluded.
- Describe verified formats and behavior; distinguish future goals from working features.

The owner has not selected a source license. Discuss licensing before submitting external implementation code.
