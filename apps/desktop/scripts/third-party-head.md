# Third-party notices

ytdlpd is GPL-3.0-or-later (see LICENSE). It ships or downloads the components below.

## Shipped inside the app

| Component | License | Notes |
|---|---|---|
| yt-dlp | Unlicense (public domain) | embedded in the Python runtime (`python/site`), pinned in `runtime.lock.json` |
| yt-dlp-ejs | Unlicense; bundles meriyah (ISC) and astring (MIT) | exact pair required by the pinned yt-dlp |
| CPython 3.14 (python-build-standalone, astral-sh) | PSF-2.0; the runtime tree also contains the licenses of its bundled libraries (OpenSSL Apache-2.0, zlib, bzip2, xz, libffi MIT, SQLite public domain, expat MIT, mpdecimal BSD-2) | see `python/LICENSE.txt` in the install |
| certifi | MPL-2.0 | CA bundle |
| requests | Apache-2.0 | |
| urllib3 | MIT | |
| charset-normalizer | MIT | |
| idna | BSD-3-Clause | |
| websockets | BSD-3-Clause | |
| mutagen | GPL-2.0-or-later | compatible with this app's GPL-3.0-or-later |
| brotli (Python bindings) | MIT | |
| pycryptodomex | BSD-2-Clause and public domain | |
| IBM Plex Sans and Mono | SIL OFL-1.1 | `assets/fonts/LICENSE.txt` |
| Lucide icons | ISC | `assets/icons/LICENSE` |
| GPUI (vendored, `packages/gpui`) | Apache-2.0 | from the Zed project |

## Downloaded at first run (not redistributed by ytdlpd)

- FFmpeg (GPL build, for example BtbN/Gyan builds): GPL-3.0 or GPL-2.0-or-later depending on the build. ytdlpd downloads it on the user's machine and runs it as a separate process; it is not linked into ytdlpd. Source and license: https://ffmpeg.org/legal.html and the build provider's release page.
- Deno: MIT, https://github.com/denoland/deno. Downloaded and run as a separate process for yt-dlp's JavaScript challenges.
