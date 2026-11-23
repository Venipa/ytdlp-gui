# ytdlpd design

Direction name: **Ledger**. Dark is the default theme (owner's rule, D-034); light ("card stock") is a first-class second theme built in Phase 0b.

## Direction
A downloads list is a ledger: numbered lines, ruled, written once and kept. Every row carries a **call number** (the job id, `0128`) in a fixed left column, and every row sits on a **ruled line**. The ruled line is the progress: a dotted track while waiting, an amber ink fill growing left to right while active, a solid quiet rule when done, a solid red rule when failed. That ruled-in ledger with call numbers is the one bold element. Everything else is quiet.

Why this is not the generic tell ("near-black plus one amber accent plus mono data labels"):
- The bold element is structural and specific to the subject (numbered ledger lines whose rule is the progress), not a coloured accent or a hero input. There is no progress bar widget, no glow, no card grid.
- Amber is reserved for things that are moving. Buttons are not amber: the primary button is paper on umber (the same quiet inversion maple uses). Selection, focus and links use a second role, ink-blue, so the UI is not "grey plus one orange".
- Mono is limited to call numbers, ids and format codes, logs and the intake text. Sizes, speeds, durations and percentages are IBM Plex Sans with tabular figures (`ui::tabular()`), so columns align without the mono look.
- Radius is 2px, not the bubbly default; hairlines and rules do the structure instead of filled cards.
- Honest caveat: dark umber plus amber is still a familiar palette family. What carries the identity is the ledger structure; if the owner dislikes the palette, swap the six colour roles below without touching any layout (D-034 is flagged).

Grounding: tokens map onto the maple `Theme` fields (verified in `crates/ui/src/theme.rs`); components come from the maple `ui` crate where they exist (list below); `ui::tabular()` and `Rounding`/`ThemeOverrides.radius` exist.

## Tokens
Mapped to maple `ui::Theme` fields. No new Theme fields (D-039); ink-blue uses `table_active_border` as its solid field (accessor `views::ink(cx)`).

| Role | Dark | Light | Theme fields |
|---|---|---|---|
| Umber / card stock (ground) | `#15130F` | `#F2EDE3` | `background`, `sidebar` |
| Drawer (raised surface) | `#1E1B16` | `#E8E1D4` | `secondary`, `popover`, `table_head` |
| Drawer edge | `#2A2620` | `#DCD3C3` | `muted`, `title_bar_border`, `sidebar_border`, `table_row_border` |
| Paper / ink (text) | `#ECE5D8` | `#1D1A15` | `foreground`, `popover_foreground`, `overlay_foreground` |
| Pencil (secondary text) | `#9C9384` | `#6B6253` | `muted_foreground`, `table_head_foreground` |
| Primary button (inverse) | `#ECE5D8` fg `#15130F` | `#1D1A15` fg `#F2EDE3` | `primary`, `primary_foreground` |
| Signal amber (activity only) | `#E9A23B` | `#A86400` | `progress_bar` |
| Ink-blue (selection, focus, links) | `#8FB0D6` | `#2F4A6B` | `table_active_border` (solid); `selection` = ink at 30%; `table_active` = ink at 12%; `sidebar_accent` = ink at 12% |
| Redaction red (danger) | `#E0584A` | `#B3261E` | `danger`; fill variant `#4A1E19` / `#F6D9D4` as row wash; `danger_foreground` `#1A0B09` / `#FFFFFF` |
Derived alphas: `border` = paper at 14%; `secondary_hover` = paper at 7%; `secondary_active` = paper at 11%; `table_hover` = paper at 5%; `overlay` = black at 55%.
Contrast: a unit test in `ui` (D-036) computes WCAG ratios from these tokens and fails below 4.5:1 for every text pair (paper/pencil/ink on ground and drawer, danger on ground, primary_foreground on primary), 3:1 for non-text (rules, focus ring). Expected from the palette: paper on umber about 14:1, pencil on umber about 6:1; amber is used for fills, rules and 13px medium text only. Verify numerically in Phase 0b; adjust the hex, not the test.
Open item for Phase 0b: check which field `ui` reads for the focus ring and make it ink-blue (amber means activity, so it must not be the focus colour); check that the danger button variant reads `danger` as its fill.

## Type
- Families (OFL, embedded via `Assets::load_fonts`, files in `apps/desktop/assets/fonts`): `IBMPlexSans-Regular.ttf`, `IBMPlexSans-Medium.ttf`, `IBMPlexSans-SemiBold.ttf`, `IBMPlexMono-Regular.ttf`, `IBMPlexMono-Medium.ttf`.
- `UI_FONT = "IBM Plex Sans"` in `views/src/root.rs`; `ui::MONO = "IBM Plex Mono"` (new const).
- Tabular figures: every number (size, speed, ETA, duration, percentage, counts) uses `.font_features(ui::tabular())`.
- Mono only for: call numbers, ids and format codes (`137+140`), logs, file paths in the detail panel, the intake text.
- Scale (theme font_size 12, the default): 10 tiny (pills, badges), 11 meta (pencil), 12 body, 14 row title and card title (Medium), 20 page title (SemiBold). Intake text 12 mono. Line height `LEADING` 1.25 as maple. Nothing renders under 10.

## Metrics
Built on maple `Metrics::new(14)`. Radius 2px via `ThemeOverrides.radius` (rows, inputs, cards, buttons; D-037, fallback 4 if buttons look cut). Hairlines 1px.
| Element | Size |
|---|---|
| Title bar | 40 (`title_bar`) |
| Page padding | 16 (`pad`) |
| Intake field | 36 high (`field`), full width, its Download button the same height |
| Controls | `control_small` 24 (icon buttons over rows), `control` 28 (buttons, chips, inputs, pickers, steppers, search), `field` 36; one height per row (D-180) |
| Page gutter | 16 (`gutter`) left and right of every page; lists run edge to edge with rows padded by the gutter |
| Download row | 52 (two lines) plus 2px ledger rule; compact mode 36 without thumbnail |
| Call-number column | 56 wide |
| Settings and logs row | 32 (`list_row`) |
| Detail panel | 360 wide, right `Panel`/`Side`, resizable |
| Thumbnail | 16:9; 80x45 in rows, 320x180 in the result card |
| Status bar | 28 |

## Ledger rule states
| Row state | Rule under the row | Row text |
|---|---|---|
| queued, paused | dotted 1px in `border` colour | pencil status ("Waiting", "Paused") |
| probing | dotted, tail dots pulse (reduced-motion: static) | "Reading the page" |
| downloading, processing | dotted track with a 2px amber fill from the left to the progress point | amber percent, tabular |
| completed | solid 1px in `border` colour | quiet; ink-blue only when selected |
| failed | solid 1px `danger` | danger text and icon, plus a one-line cause and one action |
| cancelled | dotted | pencil "Cancelled" |
Gpui dashed-border support is unverified: Phase 0b spike; fallback is a custom element painting 1px quads.

## Motion
maple `motion` presets only. Result card unfold 160 ms ease-out (`Unfold`); ledger fill animates only between ticks (linear 250 ms); toast in and out as maple. Reduced motion disables the unfold, pulse and fill easing. Nothing else moves. Spinner: maple `ui::Spinner` at default size where something loads.

## Copy rules
- Sentence case everywhere. No ALL-CAPS eyebrows. No middle-dot meta strings: metadata lives in separate aligned columns. No arrow-suffixed buttons. Real verbs, say what happened and what to do.
- Examples (key prefixes in plan.md):
  - Empty Downloads: "Paste a link to start" with "Or drop one here."
  - Filtered empty: "Nothing matches `type:audio size:>50mb`" with action "Clear filter"
  - Buttons: "Download", "Download 47", "Choose formats", "Show in folder", "Open", "Try again", "Pause", "Resume", "Cancel", "Delete entry", "Delete file", "Continue", "Use my own ffmpeg"
  - Status: "Waiting", "Reading the page", "Downloading", "Merging audio and video", "Converting to mp3", "Paused", "Interrupted, ready to resume", "Cancelled", "Done"
  - Tools: "ffmpeg is ready", "ffmpeg is missing. Some formats can't be merged." with action "Download ffmpeg"
  - Errors: "Sign in needed. Add cookies in Settings." ; "This video is private." ; "Not available in your country." ; "Too many requests. Trying again in 30 seconds." ; "The disk is full." ; "Can't write to this folder." ; "No connection. Try again."
  - Stopping: "Stopped". (Cancel and pause kill the process tree, so there is no "stopping after the current step" copy.)
  - Delete file confirm: "Delete `Never Gonna Give You Up [dQw4w9WgXcQ].mp4` from disk?" with "Delete file" and "Keep file"
  - Undo toast: "Entry removed" with "Undo" (5 s)
  - Missing file row: "File moved or deleted"
  - Chrome cookies: "Chrome blocks other apps from reading its cookies on Windows. Use Firefox, or export a cookies file."
- All strings via Fluent `t!`; no literals in views.

## Screens

### Getting ready (first run, `Destination::Welcome`)
```
+--------------------------------------------------------------+
|  ytdlpd                                              _  [] x |
+--------------------------------------------------------------+
|                                                              |
|   Getting ready                                              |
|   A few tools are needed before the first download.          |
|                                                              |
|   yt-dlp          2026.10.26        Ready                    |
|   ffmpeg          9.1               Downloading   41 of 88 MB|
|   ................================..........                  |
|   ffprobe         9.1               Waiting                  |
|   JavaScript      deno 2.9          Found on this computer   |
|                                                              |
|   Use my own ffmpeg...                       [ Continue ]    |
+--------------------------------------------------------------+
```
Each tool row: name, version (tabular), state; progress as the same ledger rule. Continue is disabled until yt-dlp imports and ffmpeg is ready or a path is chosen. Offline: "No connection. Try again" per tool; yt-dlp is baked so Continue stays possible without ffmpeg only after "Continue without ffmpeg" confirm. Later launches skip this screen; a missing tool shows a `Notice` banner on Downloads.

### Downloads (home)
```
+----------------------------------------------------------------------------------+
| ytdlpd    Downloads  Logs  Settings                                  _   []   x  |
+----------------------------------------------------------------------------------+
|  [ https://www.youtube.com/watch?v=dQw4w9WgXcQ                  ]    [ Download ]|  intake 40px, mono
|  +----------------------------------------------------------------------------+  |
|  | [ thumb 320x180 ]  Never Gonna Give You Up                                 |  |  result card (Unfold)
|  |                    Rick Astley                            3:33             |  |
|  |   Video  [Best] [2160] [1440] [1080] [720] [480]                           |  |
|  |   Audio  [Best] [mp3] [m4a] [opus]              Choose formats    Cancel   |  |
|  +----------------------------------------------------------------------------+  |
|  [All 128] [Active 2] [Done 121] [Failed 5]        [ search  type:audio size:>50mb ]
|  ------------------------------------------------------------------------------ |
|  0131  [th] Song title one                       mp3     4.1 MB   2.3 MB/s  0:12 |
|             youtube.com                          Downloading             62%     |
|        ....................======================  (amber fill = 62%)          |
|  0130  [th] A long lecture recording             1080p   1.2 GB   Paused         |
|        ..........................................  (dotted)                     |
|  0129  [th] Archived clip                        720p    88 MB    Yesterday      |
|        ------------------------------------------  (solid)                      |
|  0127  [th] Broken link                          -       -        Failed         |
|             Sign in needed. Add cookies in Settings.            [Try again]      |
|        ------------------------------------------  (solid red)                   |
|  ------------------------------------------------------------------------------ |
|  2 running, 1 paused                                       412 GB free on D:     |
+----------------------------------------------------------------------------------+
```
Row columns: call number (mono, pencil; ink-blue when selected), thumbnail, title (15 Medium) with host below (11 pencil), format label, size, speed or date, status. Hover shows row actions (Pause or Resume, Cancel or Try again, Show in folder, more). The list is `uniform_list` (never `Table`).

Empty state (no downloads):
```
|                         Paste a link to start                                    |
|                         Or drop one here.                                        |
```
Filtered empty: `Nothing matches type:audio size:>50mb` and `[Clear filter]`.

### Intake states
- Typing or pasting a valid link: probe starts on Enter or after paste; card shows `Skeleton` blocks then the result.
- Probe failure (inline in the card, one action): "Sign in needed. Add cookies in Settings." with [Open Settings]; "No connection. Try again." with [Try again]; probe timeout 45 s with [Cancel].
- Live or premiere: "Live, can't download yet."
- Already downloaded: the card shows "Already in your downloads" next to Download.
- Intake disabled only when yt-dlp cannot be imported (banner explains). ffmpeg missing: presets that need merging or converting are disabled with a reason tooltip.

### Batch card (multi-URL paste)
```
|  +----------------------------------------------------------------------------+  |
|  |  4 links                                         Preset [1080]  [Download 3] |  |
|  |  0  https://youtu.be/aaaaaaaaaaa   Intro to sets             Ready           |  |
|  |  0  https://youtu.be/bbbbbbbbbbb   Reading the page                          |  |
|  |  0  https://example.com/x          Already in your downloads                 |  |
|  |  0  not a link                     Skipped, not a link                       |  |
|  +----------------------------------------------------------------------------+  |
```
Probes up to 3 at a time; one shared preset row; invalid lines listed and skipped; duplicates flagged. (The leading `0` stands for each line's checkbox.)

### Playlist picker (inside the unfold)
```
|  48 videos in "Lecture series 2024"            [Select all] [Select none]        |
|  [x] 001  Introduction                                       12:04               |
|  [x] 002  Sets and maps                                      48:10               |
|  [ ] 003  Already downloaded                                 31:22  In your downloads
|  Preset [1080] applies to 47 selected                            [Download 47]   |
```
Lazy: `extract_flat` first, entries filled in as they are probed. Created rows share a `playlist_id`; the list may group them under a collapsible header later (not in v1).

### Detail panel (selection, right `Panel`, 360)
```
+---------------------------------------+
| No. 0131                         [x]  |
| [ thumbnail 16:9 ]                    |
| Song title one                        |
| https://youtube.com/watch?v=...  Copy |
| File   D:\Music\Song title one [..].mp3
|        [Show in folder] [Open]        |
| Format 251  opus 160k -> mp3          |   mono
| Options  preset audio:mp3, embed thumbnail, metadata
| Attempts 1                            |
| Error   (none)                        |
| Log                                   |
|   12:01:03 info  Downloading webpage  |   mono, last 20 lines
|   ...                                 |
| [Delete entry] [Delete file]          |
+---------------------------------------+
```

### Choose formats (Modal, `ui::Table`)
```
+--------------------------------------------------------------------------+
| Choose formats                                                    [x]    |
| Video                                                                    |
|   id    ext   resolution  codec      size      note                      |
|   137   mp4   1920x1080   avc1       112 MB    1080p                     |
|   248   webm  1920x1080   vp9        74 MB     1080p                     |
| Audio                                                                    |
|   140   m4a   -           mp4a 128k  3.4 MB    medium                    |
|   251   webm  -           opus 160k  3.7 MB    medium                    |
| Result  137+140 -> mp4                              [Cancel] [Download]  |
+--------------------------------------------------------------------------+
```
Ids and codec strings mono; sizes tabular sans. Selection rows use ink-blue.

### Settings (left `SideNav`, right form with `Form/Field/SwitchRow/Dropdown`)
```
+--------------------------------------------------------------------------+
| ytdlpd    Downloads  Logs  Settings                                      |
+----------------+---------------------------------------------------------+
|  General       |  Downloads                                              |
|  Downloads  <  |  Save to        D:\Videos\ytdlpd          [Choose]      |
|  Formats       |  File name      %(title).180B [%(id)s].%(ext)s          |
|  Network       |  Preview        Never Gonna Give You Up [dQw4w9WgXcQ].mp4
|  Tools         |  At once        [3]   Per site [2]                      |
|  Appearance    |  Retries        [5]                                     |
|  About         |  Resume interrupted downloads on start        [ off ]   |
+----------------+---------------------------------------------------------+
```
Pages and contents:
- General: language, start minimised, close to tray, notifications, clipboard watch and auto-add, resume on start.
- Downloads: output folder, templates with preview, concurrency, retries.
- Formats: default preset, subtitles (languages, embed or file), embed thumbnail, metadata, chapters, SponsorBlock (off, mark, remove).
- Network: proxy, rate limit, cookies (None, File, From browser) with the Chrome note.
- Tools: below.
- Appearance: theme (System, Dark, Light, maple themes), compact mode, font size.
- About: version, licenses, yt-dlp and tool versions, links.

Tools page:
```
|  Tools                                                                    |
|  yt-dlp     2026.10.26   Managed                    [Check for update]    |
|  ffmpeg     9.1          Managed                    [Re-download] [Choose file]
|  ffprobe    9.1          Managed                    [Re-download] [Choose file]
|  JavaScript deno 2.9.8   Found at C:\...\deno.exe   [Choose file]         |
|  Python     3.14.10      Bundled                                          |
|  Windows long paths are off. Long file names may fail.   (Notice, if off) |
```
A custom path to a non-ffmpeg executable is rejected with "That file is not ffmpeg."

### Logs
```
|  [All] [Info] [Warn] [Error]    Job [ All jobs v ]              [Copy all] |
|  12:01:03  info   0131  [youtube] Extracting URL: https://...             |   mono
|  12:01:05  warn   0131  No supported JavaScript runtime...                |
```
Empty: "No log lines yet."

### Error and edge states
- Tools missing later: `Notice` banner on Downloads with action.
- Disk full or folder not writable: row fails with "The disk is full." or "Can't write to this folder." and [Open Settings] for the folder.
- Interrupted after a crash: row paused, "Interrupted, ready to resume".

## Keyboard map
| Keys | Action |
|---|---|
| Ctrl+V on Downloads | focus intake and paste |
| Ctrl+L | focus intake |
| Enter in intake | probe, or download when the card is open |
| Esc | cancel card, close panel or modal |
| Ctrl+F | focus search |
| Up, Down | move row selection |
| Enter on a row | open file |
| Space | pause or resume selected |
| Del | delete entry (undo toast) |
| Shift+Del | delete file (confirm) |
| Ctrl+, | Settings |
| Ctrl+1, Ctrl+2, Ctrl+3 | Downloads, Logs, Settings |
Every row has `.id`, `.role`, `aria_label` and `aria-selected`; progress is announced at 10% steps, not per tick.

## UI components
Verified against `E:\git\ytm-maple-rs\apps\desktop\crates\ui\src` (file list read).
| Need | Status |
|---|---|
| Button, Card, Checkbox, Radio, Switch, SwitchRow, Dropdown, Picker, Form/Field, Input, Kbd, Label, Menu (context menu), Popover, Popup, Modal, Panel/Side (right panel), Tooltip, Toast, Notice, Skeleton, Spinner, Separator, Unfold/Cascade, Vacancy, OptionCard, InfoCard, Scroller/Scrollbar, Lazy, motion, drag/drop, `tabular()` | exist, reuse |
| Tabs | exists but horizontal only (no vertical tabs); not used for Settings |
| Table | exists, full-featured but music-shaped and heavy; used only for Choose formats |
| Virtual list | none; use gpui `uniform_list` directly (as maple) |
| `ProgressRule` (ledger rule: dotted track, amber fill, solid, danger, indeterminate pulse) | build (lane A) |
| `CallNumber` (4-digit mono) | build |
| `Thumbnail` (gpui img + Skeleton + 16:9 crop; replaces music `Artwork` use) | build |
| `Chip` (filter chip with count, selected state) | build |
| `TokenInput` (Input with token highlighting for `type:`, `size:`, `status:`, `site:`) | build |
| `SideNav` (vertical list for Settings) | build (about 1 day) |
| `Dropzone` overlay for drag and drop | build (Phase 5) |
| `MONO` const | add |
| Music-only: Artwork (kept: coupled to card, picker, table, drag), Visualizer, Waves, LevelMeter, ResponseGraph, ResponseSketch, Lattice, Rail, Brand, Deck, Scrubber, Pin, CoverPalette | copy verbatim in 0a, prune leaf ones in Phase 6 |
