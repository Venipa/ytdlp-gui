<p align="center">
  <img src="icons/ytdlpd/icon-1024.png" width="128" alt="ytdlpd app icon">
</p>

<h1 align="center">ytdlpd</h1>

<p align="center">
  Paste a link, pick a preset, get a correctly named file.<br>
  A native desktop app for anything <a href="https://github.com/yt-dlp/yt-dlp">yt-dlp</a> supports, on Windows, macOS and Linux.
</p>

<p align="center">
  <a href="https://github.com/Venipa/ytdlp-gui/releases/latest"><b>Download</b></a> ·
  <a href="https://ytdlp.venipa.net">Website</a> ·
  <a href="https://ytdlp.venipa.net/docs">Docs</a>
</p>

![The Downloads page: a link field, filter chips and a numbered list of downloads, three in progress](apps/web/assets/shots/downloads-dark.webp)

## What it does

- **Paste and go.** Paste one link or many; playlists open as a list you can trim before adding.
- **Presets that convert for real.** Video from best down to 480p, audio as mp3, m4a or opus.
- **A queue you can trust.** Three downloads at a time, two per site. Pause, resume, retry; after a restart unfinished work waits, paused.
- **Files that play everywhere.** After each download the file is checked; if it may not play in QuickTime, Photos or Windows, one click makes it compatible. Repair, loudness and smaller-file fixes are there too.
- **Site rules.** Per site (domain, URL or regex): what the clipboard watch does, which preset to use, which fix to run.
- **Cookies, kept tidy.** Import a cookies.txt, paste one, or copy a browser's; each download only gets the cookies for its own site.
- **Tools fetched for you.** ffmpeg and a JavaScript runtime download when missing; yt-dlp updates without a new app.
- **Native and light.** Rust and GPUI, not a browser in a window. Dark and light themes, plus window blur on macOS and Windows.

## Install

Grab the installer for your system from the [latest release](https://github.com/Venipa/ytdlp-gui/releases/latest):

| System | File |
|---|---|
| Windows 10/11 (x64) | `ytdlpd-<version>-windows-x64-setup.exe` |
| macOS 13+ (Apple silicon / Intel) | `ytdlpd-<version>-macos-arm64.dmg` / `-macos-x64.dmg` |
| Linux (x64 / arm64) | `ytdlpd-<version>-linux-x64.AppImage` / `-linux-arm64.AppImage` |

On first start the app fetches ffmpeg and a JavaScript runtime, then it is ready.

## Screenshots

| Many links at once | Getting ready |
|---|---|
| ![The multi-link editor](apps/web/assets/shots/multi-link-editor.webp) | ![First start: the tools being fetched](apps/web/assets/shots/getting-ready.webp) |
| **Downloads settings** | **Appearance** |
| ![Downloads settings: folder, file name template with a live preview, limits](apps/web/assets/shots/settings-downloads.webp) | ![Appearance: themes, window background and blur](apps/web/assets/shots/settings-appearance.webp) |

## Building from source

See [docs/development.md](docs/development.md) for prerequisites, running a dev build and packaging.

## License

GPL-3.0-or-later. Third-party licenses are in [THIRD-PARTY.md](THIRD-PARTY.md).
