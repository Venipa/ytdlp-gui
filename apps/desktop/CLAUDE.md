# ytdlpd desktop

A native yt-dlp downloader. Rust and GPUI (vendored in `packages/gpui`). Cargo workspace, edition 2024, resolver 3.
Plan, architecture and design live in `../../docs/` (`plan.md` first). Owner decisions are final in `docs/decisions.md`.

## Crate layout and dependency direction

```
ytdlpd (bin) -> views -> state -> {engine, tools, storage, domain}
ytdlpd, views, state -> {ui, router, input, i18n, icons}
router, input -> ui -> {i18n, icons} -> gpui
```
- `engine`, `tools`, `storage`, `domain` are headless: no gpui, no ui. No back edges.
- `domain` is the contract crate; it is frozen at tag `contract-1`. Changes use a `CONTRACT:` commit prefix.
- `ytdlpd/src/main.rs` exposes `build_app(...)`; state init plugs in there. Views never edit it.

## Lanes (parallel work)
A UI (`ui views router input icons`, themes, fonts), B engine (`engine`, `scripts/setup.ts`, runtime), C data and state (`domain storage state`), D tools and platform (`tools cli ytdlpd` bin, packaging, CI). Ownership table: `docs/plan.md`.

## Rules
- Debug builds use `ytdlpd.dev` data dirs: take names from `storage::app_dir_name()`, never write a `"ytdlpd"` path literal.
- Never hardcode a color, radius or size: use `cx.theme()` and `theme.metrics`; new tokens go in `Theme`.
- Every user-facing string comes from Fluent `t!` (`assets/i18n/en-US/`, key prefixes `dl-`, `set-`, `welcome-`, `toast-`, `err-`, `tools-`). Never call `t!` in a constructor: resolve in `render`.
- Every interactive element needs `.id()` plus `.role()` and an Fluent `aria_label`. Extend `crates/ui/tests/a11y_tree.rs` with each new primitive.
- Network runs on tokio, never on the gpui executor. Store `Task`s in fields.
- `///` on every pub item; `anyhow` `.context("cannot ...")` lowercase; logs prefixed by subsystem (`"engine: ..."`).
- Smallest complete change; non-trivial logic gets a unit test; known limits get `// shortcut: <limit>, <when to upgrade>`.
- Dependencies are declared once in `[workspace.dependencies]` of `Cargo.toml` (lane blocks `# lane X`).

## Handoff rule
Context diet: at about 300k tokens stop new work and write `F:\tmp\ytdlpd-rs\handoff\<lane>.md` (state, open items, files owned, next steps and commands, pitfalls). Never exceed 350k.

## Commands (from `apps/desktop`)
```sh
bun run dev                   # from repo root: cargo run -p ytdlpd
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked        # cargo-nextest if installed
```

## Windows temp rule
Never use C: for temp. Set `TMP`, `TEMP` and `CARGO_TARGET_DIR` under `F:\tmp\ytdlpd-rs\` for every cargo command (first gpui build takes 10 to 20 minutes).

## Commits
Conventional Commits, subject only. Do not push; the lead pushes.
