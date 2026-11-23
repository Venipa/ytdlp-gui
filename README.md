# ytdlpd

A native desktop downloader for yt-dlp, written in Rust with GPUI. GPL-3.0-or-later.

Status: Phase 0a skeleton, see `docs/plan.md`.

## Develop
```
bun run dev      # cargo run -p ytdlpd
bun run check    # cargo check --workspace
cd apps/desktop && cargo test --workspace
```
Layout: `apps/desktop` (workspace, crates in `crates/`), `packages/gpui` (vendored GPUI), `docs/` (plan, architecture, design).
