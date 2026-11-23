# Changelog

## Unreleased
- Packaging: Windows Inno Setup installer (python runtime beside the exe, `ytdlpd://` registration, longPathAware manifest), macOS `.app`/dmg and Linux AppImage scripts, tag-triggered 6-target release workflow, `THIRD-PARTY.md`.
- Dev setup: `bun run setup` works on macOS and Linux (stdlib path, pruning), mold is opt-in, CI runs setup before building.
- Project skeleton: GPUI app shell with title-bar navigation (Downloads, Logs, Settings).
