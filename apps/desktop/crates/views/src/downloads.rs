//! The Downloads page over the state entities: intake and result card, the pinned "In progress"
//! group over the history, filter chips, the detail panel and the status bar.

use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use std::str::FromStr as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use domain::{
    Chip as StatusChip, DownloadId, DownloadStatus, DownloadSummary, EngineError, ErrorCode,
    Filter, Preset, ProbeOpts, ProbeResult, Term, ToolState, split_urls,
};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, ExternalPaths, FocusHandle, Focusable as _,
    MouseButton, Pixels, Point, ScrollStrategy, SharedString, Subscription,
    UniformListScrollHandle, Window, div, px, uniform_list,
};
use i18n::t;
use state::{AppState, DownloadsEvent, IntakeEvent, ProbeState};
use ui::{
    ActiveTheme as _, Button, Checkbox, Chip, FORM_CONTEXT, Input, Kbd, Scrollbar, Scroller,
    Submit, Text, TextArea, TokenInput, Vacancy,
};

use crate::format;
use crate::formats::Draft;
use crate::os;
use crate::toasts::{Kind, toast};
use input::ROWS_CONTEXT;

/// How long a deleted entry can still be brought back.
const UNDO: Duration = Duration::from_secs(6);
/// How long a new row takes to fade in.
pub(crate) const FADE: Duration = Duration::from_millis(160);
/// How often the free space of the output drive is read again.
const FREE_EVERY: Duration = Duration::from_secs(5);
/// How many lines the editor of several links shows before it scrolls.
const EDITOR_LINES: f32 = 8.;

/// What the result card under the intake shows.
#[derive(Clone)]
pub enum Card {
    None,
    Probing,
    /// One video, probed.
    Result(Arc<ProbeResult>),
    /// Several links.
    Batch,
    /// One link that is a playlist.
    Playlist(Arc<ProbeResult>),
    /// A probe failure.
    Failed(EngineError),
    /// The text holds no link.
    NotLink,
}

/// One line of the list: a group header or a download (its index in the state rows).
#[derive(Clone, Copy)]
pub enum Line {
    Header(&'static str, usize),
    Row(usize),
}

/// A keyboard action of the page, as forwarded by the Workspace.
#[derive(Clone, Copy, PartialEq)]
pub enum Key {
    Next,
    Previous,
    TogglePause,
    DeleteEntry,
    DeleteFile,
    Deselect,
    Paste,
    FocusIntake,
    FocusSearch,
    First,
    Last,
    Open,
    TickAll,
    TickNext,
    TickPrevious,
}

/// What the selection bar does to the ticked rows; each acts on the rows it fits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Bulk {
    Pause,
    Resume,
    Retry,
    Cancel,
    Copy,
    Remove,
    Delete,
}

impl Bulk {
    /// Whether the action applies to a row in this state.
    pub(crate) fn fits(self, s: &DownloadSummary) -> bool {
        let status = s.status;
        match self {
            Bulk::Pause => !status.is_terminal() && status != DownloadStatus::Paused,
            Bulk::Resume => status == DownloadStatus::Paused,
            Bulk::Retry => matches!(status, DownloadStatus::Failed | DownloadStatus::Cancelled),
            Bulk::Cancel => !status.is_terminal(),
            Bulk::Copy => true,
            Bulk::Remove => !status.is_active(),
            Bulk::Delete => !status.is_active() && s.file_path.is_some(),
        }
    }
}

/// The Downloads page.
pub struct Downloads {
    pub(crate) app: AppState,
    pub(crate) intake: Entity<TextArea>,
    pub(crate) search: Entity<Input>,
    pub(crate) chip: StatusChip,
    pub(crate) selected: Option<DownloadId>,
    /// The preset the card will download with.
    pub(crate) preset: Preset,
    /// The preset of the last download, which the card says it remembers.
    pub(crate) remembered: Preset,
    /// The links the card is about.
    pub(crate) urls: Vec<String>,
    /// Pasted words that are not links.
    pub(crate) skipped: Vec<String>,
    /// Enter was pressed while the page was still being read.
    enter_waits: bool,
    /// The editor of several links is open over the list.
    pub(crate) editing: bool,
    last_text: String,
    /// The open Choose formats modal.
    pub(crate) formats: Option<Draft>,
    /// The scroll of the video and the audio list of the formats modal.
    pub(crate) format_scroll: [gpui::ScrollHandle; 2],
    /// Playlist entries the user switched off.
    pub(crate) playlist_off: HashSet<u32>,
    /// A right-click menu: where, and for which row.
    pub(crate) menu: Option<(Point<Pixels>, DownloadId)>,
    /// A delete-file confirmation.
    pub(crate) confirm: Option<DownloadId>,
    /// The rows ticked for a bulk action.
    pub(crate) ticks: crate::ticks::Ticks,
    /// A bulk removal (`false`) or file deletion (`true`) waiting for its one confirmation.
    pub(crate) bulk_confirm: Option<(bool, Vec<DownloadId>)>,
    /// Entries hidden while their undo toast is up.
    pub(crate) hidden: HashSet<DownloadId>,
    /// The ids in list order, for the arrow keys.
    pub(crate) order: Vec<DownloadId>,
    /// The list line of each id in `order`.
    order_lines: Vec<usize>,
    pub(crate) scroll: UniformListScrollHandle,
    bar: Entity<Scrollbar>,
    /// The list takes the keys that act on its selected row while it holds focus.
    pub(crate) list_focus: FocusHandle,
    /// Scroll state of the detail panel and of its log tail.
    pub(crate) detail_bar: Entity<Scrollbar>,
    pub(crate) log_bar: Entity<Scrollbar>,
    /// Whether the detail panel's Details section is open.
    pub(crate) details_open: bool,
    /// Whether an empty library already moved focus to the intake.
    intake_offered: bool,
    /// The page's width as last laid out; the list's columns and where the details go follow it.
    page_width: Pixels,
    /// Links on the clipboard, offered in a strip under the link field until added or put away.
    clip_offer: Option<Vec<String>>,
    /// Every id the list has shown, and the ones that arrived lately and fade in.
    known: HashSet<DownloadId>,
    pub(crate) fresh: std::collections::HashMap<DownloadId, Instant>,
    free: Option<(u64, String)>,
    free_at: Option<Instant>,
    /// A `--screen` scene still waiting for rows or a probe (`detail`, `failed-detail`, `formats`).
    scene: Option<&'static str>,
    _subs: Vec<Subscription>,
}

impl Downloads {
    /// Builds the page over the registered state.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let intake = cx.new(|cx| {
            TextArea::new("dl-intake-hint", cx)
                .name("dl-intake-name")
                .icon("icons/link.svg")
                // one line tall in the layout; several lines open the editor over the list
                .max_lines(1.)
                .clearable()
        });
        let search = cx.new(|cx| {
            Input::new(t!("dl-search-hint"), cx)
                .name("dl-search-name")
                .icon("icons/search.svg")
                .shortcut("/")
                .clearable()
        });
        let list_focus = cx.focus_handle().tab_stop(true);
        let subs = vec![
            // Tab into the list brings its selected row into sight, so the ring shows on it
            cx.on_focus(&list_focus, window, |this: &mut Self, _, cx| {
                if let Some(line) = this.selected.and_then(|id| this.line_of(id)) {
                    this.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
                    cx.notify();
                }
            }),
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe_in(&intake, window, |this, _, window, cx| {
                // only an edit opens it: Esc folds it while the field keeps focus
                if this.intake_changed(cx) {
                    this.editor_follows(window, cx);
                }
            }),
            cx.on_focus(&intake.focus_handle(cx), window, Self::editor_follows),
            cx.subscribe(&intake, |this, _, _: &ui::Pasted, cx| this.pasted(cx)),
            cx.observe(&app.downloads, |_, _, cx| cx.notify()),
            cx.observe(&app.fixes, |_, _, cx| cx.notify()),
            cx.observe(&app.settings, |_, _, cx| cx.notify()),
            cx.observe(&app.tools, |_, _, cx| cx.notify()),
            cx.observe(&app.logs, |_, _, cx| cx.notify()),
            cx.observe(&app.probes, |this, _, cx| this.probes_changed(cx)),
            cx.subscribe_in(&app.downloads, window, Self::download_event),
            cx.subscribe_in(&app.intake, window, Self::intake_event),
        ];
        let remembered =
            Preset::from_str(&app.settings.read(cx).get().last_preset).unwrap_or(Preset::DEFAULT);
        let scroll = UniformListScrollHandle::new();
        let bar = cx.new(|cx| {
            Scrollbar::new(scroll.0.borrow().base_handle.clone()).watching(cx.entity_id())
        });
        let detail_bar =
            cx.new(|cx| Scrollbar::new(gpui::ScrollHandle::new()).watching(cx.entity_id()));
        let log_bar =
            cx.new(|cx| Scrollbar::new(gpui::ScrollHandle::new()).watching(cx.entity_id()));
        let mut this = Self {
            list_focus,
            detail_bar,
            log_bar,
            details_open: true,
            intake_offered: false,
            page_width: Pixels::ZERO,
            clip_offer: None,
            known: HashSet::new(),
            fresh: std::collections::HashMap::new(),
            app,
            intake,
            search,
            chip: StatusChip::All,
            selected: None,
            preset: remembered,
            remembered,
            urls: Vec::new(),
            skipped: Vec::new(),
            enter_waits: false,
            editing: false,
            last_text: String::new(),
            formats: None,
            format_scroll: Default::default(),
            playlist_off: HashSet::new(),
            menu: None,
            confirm: None,
            ticks: Default::default(),
            bulk_confirm: None,
            hidden: HashSet::new(),
            order: Vec::new(),
            order_lines: Vec::new(),
            bar,
            scroll,
            free: None,
            free_at: None,
            scene: None,
            _subs: subs,
        };
        // links that arrived before the window (command line, deep link)
        let waiting = this.app.intake.update(cx, |i, _| i.take_pending());
        if !waiting.is_empty() {
            this.fill(&waiting, cx);
        }
        this
    }

    // ---- intake -------------------------------------------------------------------------

    /// Adds links to the field, after what it already holds, and starts reading them.
    pub(crate) fn fill(&mut self, urls: &[String], cx: &mut Context<Self>) {
        let text = appended(self.intake.read(cx).text(), urls.iter().map(String::as_str));
        self.last_text.clone_from(&text);
        self.intake.update(cx, |input, cx| input.set_text(text, cx));
        self.begin(cx);
    }

    /// Debug `--screen` states of this page for screenshots (pair with `--demo mixed`): `card`,
    /// `probing`, `batch`, `playlist`, `error`, `formats`, `detail`, `failed-detail`, `filtered`.
    pub fn show_scene(&mut self, name: &str, cx: &mut Context<Self>) {
        let link = |s: &str| vec![s.to_string()];
        match name {
            "card" => self.fill(&link("https://www.youtube.com/watch?v=jNQXAC9IVRw"), cx),
            "probing" => {
                let url = "https://www.youtube.com/watch?v=jNQXAC9IVRw".to_string();
                self.last_text.clone_from(&url);
                self.intake
                    .update(cx, |input, cx| input.set_text(url.clone(), cx));
                // no probe is requested, so the card stays on its reading state
                self.urls = vec![url];
            }
            "batch" => self.fill(
                &[
                    "https://www.youtube.com/watch?v=aqz-KE-bpKQ",
                    "https://vimeo.com/76979871",
                    "https://www.youtube.com/watch?v=demo00008",
                    "https://example.com/probefail",
                    "not-a-link",
                ]
                .map(String::from),
                cx,
            ),
            "playlist" => self.fill(&link("https://www.youtube.com/playlist?list=PLdemo"), cx),
            "error" => self.fill(&link("https://www.youtube.com/watch?v=private"), cx),
            "formats" => {
                self.scene = Some("formats");
                self.fill(&link("https://www.youtube.com/watch?v=jNQXAC9IVRw"), cx);
            }
            "detail" => self.scene = Some("detail"),
            "failed-detail" => self.scene = Some("failed-detail"),
            "filtered" => self
                .search
                .update(cx, |input, cx| input.set_text("type:audio size:>5gb", cx)),
            _ => {}
        }
        self.scene_rows(cx);
    }

    /// Selects the row a pending `detail` scene wants once the rows are in.
    fn scene_rows(&mut self, cx: &mut Context<Self>) {
        let want = match self.scene {
            Some("detail") => DownloadStatus::Completed,
            Some("failed-detail") => DownloadStatus::Failed,
            _ => return,
        };
        let downloads = self.app.downloads.read(cx);
        if !downloads.loaded() {
            return;
        }
        self.selected = downloads
            .rows()
            .iter()
            .find(|r| r.status == want)
            .map(|r| r.id);
        self.scene = None;
        cx.notify();
    }

    fn intake_event(
        &mut self,
        _: &Entity<state::Intake>,
        event: &IntakeEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            IntakeEvent::Add(_) => {
                let urls = self.app.intake.update(cx, |i, _| i.take_pending());
                self.fill(&urls, cx);
            }
            IntakeEvent::Offer(urls) => {
                self.clip_offer = Some(urls.clone());
                cx.notify();
            }
        }
    }

    /// Pasted links go one to a line, as the old app's box had them, and are read at once.
    fn pasted(&mut self, cx: &mut Context<Self>) {
        let lined = one_per_line(self.intake.read(cx).text());
        self.last_text.clone_from(&lined);
        self.intake.update(cx, |area, cx| area.set_text(lined, cx));
        self.begin(cx);
    }

    /// Typing edits a link, pasting starts reading it.
    /// Whether the text changed (a caret move notifies too).
    fn intake_changed(&mut self, cx: &mut Context<Self>) -> bool {
        let text = self.intake.read(cx).text().to_owned();
        if text == self.last_text {
            return false;
        }
        let pasted = text.len() > self.last_text.len() + 1;
        self.last_text.clone_from(&text);
        if text.trim().is_empty() {
            self.clear_card(cx);
            self.close_editor(cx);
        } else if pasted {
            self.begin(cx);
        } else if !self.urls.is_empty() && !self.editing {
            // the editor keeps what was read while lines are typed; Enter reads the new ones
            self.clear_card(cx);
        }
        cx.notify();
        true
    }

    /// Opens the editor while the focused field holds more than one line (owner pick 3A: on
    /// focus, on Shift+Enter, on a paste of several links).
    fn editor_follows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let several = self.intake.read(cx).text().contains('\n');
        if several && !self.editing && self.intake.focus_handle(cx).is_focused(window) {
            self.open_editor(window, cx);
        }
    }

    pub(crate) fn open_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = true;
        self.intake.update(cx, |area, cx| {
            area.set_max_lines(EDITOR_LINES, cx);
            area.focus(window, cx);
        });
        cx.notify();
    }

    /// Folds the editor back into the one-line field, which shows the first link again.
    pub(crate) fn close_editor(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.editing) {
            return;
        }
        self.intake.update(cx, |area, cx| {
            area.set_notes(None, cx);
            area.set_max_lines(1., cx);
            area.move_to_start(cx);
        });
        cx.notify();
    }

    /// Whether a link of the card is still being read.
    fn reading(&self, cx: &App) -> bool {
        let probes = self.app.probes.read(cx);
        self.urls
            .iter()
            .any(|url| matches!(probes.get(url), None | Some(ProbeState::Loading)))
    }

    fn probe_opts(&self, cx: &App) -> ProbeOpts {
        let settings = self.app.settings.read(cx).get();
        ProbeOpts {
            timeout_s: None,
            flat_playlist: true,
            cookies: Some(settings.cookies.clone()),
            proxy: settings.proxy.clone(),
            // the deno set in settings, else the detected one, as downloads get it (AppState::job_spec)
            js_runtime: settings
                .tool_paths
                .deno
                .clone()
                .or_else(|| {
                    self.app
                        .tools
                        .read(cx)
                        .get(domain::ToolKind::Deno)
                        .and_then(|s| s.path.clone())
                })
                .map(|p| ("deno".to_string(), p)),
        }
    }

    /// Reads the field: every link is probed and the card follows the results.
    pub(crate) fn begin(&mut self, cx: &mut Context<Self>) {
        let text = self.intake.read(cx).text().to_owned();
        let split = split_urls(&text);
        // a link that stays keeps its probe (a failed one is read again)
        for url in std::mem::take(&mut self.urls) {
            if !split.urls.contains(&url) {
                self.app.probes.update(cx, |p, cx| p.remove(&url, cx));
            }
        }
        self.enter_waits = false;
        self.skipped = split.invalid.into_iter().map(|(_, word)| word).collect();
        self.urls = split.urls;
        self.playlist_off.clear();
        self.remembered = Preset::from_str(&self.app.settings.read(cx).get().last_preset)
            .unwrap_or(Preset::DEFAULT);
        self.preset = self.remembered;
        // one link a site rule gives a preset: the chip shows that preset (still changeable)
        if let [url] = self.urls.as_slice()
            && let Some(preset) = self.rule_preset(url, cx)
        {
            self.preset = preset;
        }
        let opts = self.probe_opts(cx);
        for url in self.urls.clone() {
            self.app
                .probes
                .update(cx, |p, cx| p.request(&url, opts.clone(), cx));
        }
        cx.notify();
    }

    /// Forgets the probes of the card's links.
    fn release(&mut self, cx: &mut Context<Self>) {
        for url in std::mem::take(&mut self.urls) {
            self.app.probes.update(cx, |p, cx| p.remove(&url, cx));
        }
        self.enter_waits = false;
    }

    /// Closes the card (the field keeps its text unless the caller clears it).
    pub(crate) fn clear_card(&mut self, cx: &mut Context<Self>) {
        self.release(cx);
        self.skipped.clear();
        self.formats = None;
        cx.notify();
    }

    /// The card for the current links and probe results.
    pub(crate) fn card(&self, cx: &App) -> Card {
        let probes = self.app.probes.read(cx);
        match self.urls.as_slice() {
            [] if self.skipped.is_empty() => Card::None,
            [] => Card::NotLink,
            [url] => match probes.get(url) {
                None | Some(ProbeState::Loading) => Card::Probing,
                Some(ProbeState::Failed(error)) => Card::Failed(error.clone()),
                Some(ProbeState::Ready(p)) if p.is_playlist() => Card::Playlist(p.clone()),
                Some(ProbeState::Ready(p)) => Card::Result(p.clone()),
            },
            _ => Card::Batch,
        }
    }

    fn probes_changed(&mut self, cx: &mut Context<Self>) {
        let settled = match self.card(cx) {
            Card::Result(_) => true,
            Card::Batch => !self.reading(cx),
            _ => false,
        };
        if self.enter_waits && settled {
            self.enter_waits = false;
            self.download(cx);
        }
        if self.scene == Some("formats") && matches!(self.card(cx), Card::Result(_)) {
            self.scene = None;
            self.open_formats(cx);
        }
        cx.notify();
    }

    /// Enter in the field and the Download button.
    pub(crate) fn submit(&mut self, cx: &mut Context<Self>) {
        // links typed in the editor since the last read are read first, then downloaded
        let links = split_urls(self.intake.read(cx).text()).urls;
        if (self.editing || links.len() > 1) && !links.is_empty() && links != self.urls {
            self.begin(cx);
            self.enter_waits = true;
            return;
        }
        match self.card(cx) {
            Card::None | Card::NotLink => self.begin(cx),
            Card::Probing => self.enter_waits = true,
            Card::Failed(_) => {
                // read the page again
                let urls = self.urls.clone();
                self.fill(&urls, cx);
            }
            Card::Batch if self.reading(cx) => self.enter_waits = true,
            Card::Result(_) | Card::Batch | Card::Playlist(_) => self.download(cx),
        }
    }

    /// Whether `url` is already a row.
    pub(crate) fn have(&self, url: &str, cx: &App) -> bool {
        self.app
            .downloads
            .read(cx)
            .rows()
            .iter()
            .any(|r| r.url == url)
    }

    /// The preset the first matching site rule sets for `url`, if it sets one.
    fn rule_preset(&self, url: &str, cx: &App) -> Option<Preset> {
        let rules = &self.app.settings.read(cx).get().site_rules;
        domain::rule_for(rules, url).and_then(|(_, rule)| rule.preset)
    }

    /// The preset `url` is added with: a preset the user picked wins; while the chip still shows
    /// the remembered default, a site rule's preset applies.
    fn preset_for(&self, url: &str, cx: &App) -> Preset {
        match self.preset == self.remembered {
            true => self.rule_preset(url, cx).unwrap_or(self.preset),
            false => self.preset,
        }
    }

    /// Adds what the card shows with the chosen preset.
    pub(crate) fn download(&mut self, cx: &mut Context<Self>) {
        let preset = self.preset;
        let added = match self.card(cx) {
            Card::Result(p) => {
                self.enqueue(&p.url, preset, "", None, Some(&p), cx);
                1
            }
            Card::Batch => {
                let ready: Vec<Arc<ProbeResult>> = self
                    .urls
                    .iter()
                    .filter(|url| !self.have(url, cx))
                    .filter_map(|url| match self.app.probes.read(cx).get(url) {
                        Some(ProbeState::Ready(p)) if !p.is_playlist() => Some(p.clone()),
                        _ => None,
                    })
                    .collect();
                for p in &ready {
                    let preset = self.preset_for(&p.url, cx);
                    self.enqueue(&p.url, preset, "", None, Some(p), cx);
                }
                ready.len()
            }
            Card::Playlist(p) => {
                let page = p.webpage_url.clone().unwrap_or_else(|| p.url.clone());
                let entries = p.entries.clone().unwrap_or_default();
                let mut count = 0;
                let off = self.playlist_off.clone();
                let preset = self.preset_for(&page, cx);
                for entry in entries.iter().filter(|e| !off.contains(&e.index)) {
                    let stub = ProbeResult {
                        url: entry.url.clone(),
                        webpage_url: Some(entry.url.clone()),
                        id: entry.id.clone(),
                        title: entry.title.clone(),
                        duration_s: entry.duration_s,
                        thumbnail_url: entry.thumbnail_url.clone(),
                        ..Default::default()
                    };
                    self.enqueue(&page, preset, "", Some(entry.index), Some(&stub), cx);
                    count += 1;
                }
                count
            }
            _ => 0,
        };
        if added == 0 {
            return;
        }
        // a preset a site rule chose is not remembered as the user's own
        let from_rule =
            matches!(self.urls.as_slice(), [url] if self.rule_preset(url, cx) == Some(preset));
        if preset != Preset::Custom && !from_rule {
            self.app.settings.update(cx, |s, cx| {
                s.update(|s| s.last_preset = preset.to_string(), cx)
            });
        }
        toast(
            cx,
            Kind::Success,
            match added {
                1 => t!("dl-toast-added-one"),
                n => t!("dl-toast-added-many", count = n),
            },
            None,
        );
        self.intake.update(cx, |input, cx| input.set_text("", cx));
        self.last_text.clear();
        self.clear_card(cx);
        self.close_editor(cx);
    }

    /// Adds one download; `format_spec` is for the Custom preset.
    pub(crate) fn enqueue(
        &mut self,
        url: &str,
        preset: Preset,
        format_spec: &str,
        playlist_index: Option<u32>,
        probe: Option<&ProbeResult>,
        cx: &mut Context<Self>,
    ) {
        let label = match preset {
            Preset::Custom => self.formats_label(probe, format_spec),
            other => format::preset_label(other),
        };
        let task = self.app.enqueue_job(
            url,
            preset,
            format_spec,
            playlist_index,
            probe,
            Some(label),
            cx,
        );
        task.detach_and_log_err(cx);
    }

    // ---- row actions ---------------------------------------------------------------------

    pub(crate) fn summary(&self, id: DownloadId, cx: &App) -> Option<DownloadSummary> {
        self.app.downloads.read(cx).get(id).cloned()
    }

    pub(crate) fn pause(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.app.downloads.update(cx, |d, cx| d.pause(id, cx));
    }

    pub(crate) fn resume(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.app.downloads.update(cx, |d, cx| d.resume(id, cx));
    }

    /// Pauses every running and waiting row, or (`resume`) resumes every paused one.
    fn all(&mut self, resume: bool, cx: &mut Context<Self>) {
        self.app.downloads.update(cx, |d, cx| {
            let ids: Vec<DownloadId> = d
                .rows()
                .iter()
                .filter(|r| match resume {
                    true => r.status == DownloadStatus::Paused,
                    false => !r.status.is_terminal() && r.status != DownloadStatus::Paused,
                })
                .map(|r| r.id)
                .collect();
            for id in ids {
                match resume {
                    true => d.resume(id, cx),
                    false => d.pause(id, cx),
                }
            }
        });
    }

    pub(crate) fn cancel(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.app.downloads.update(cx, |d, cx| d.cancel(id, cx));
    }

    pub(crate) fn retry(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.app.downloads.update(cx, |d, cx| d.retry(id, cx));
    }

    /// Space on a row: pause what runs, resume what is paused.
    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        match self.summary(id, cx).map(|s| s.status) {
            Some(DownloadStatus::Paused) => self.resume(id, cx),
            Some(s) if !s.is_terminal() => self.pause(id, cx),
            _ => {}
        }
    }

    pub(crate) fn show_folder(&self, id: DownloadId, cx: &App) {
        match self.summary(id, cx).and_then(|s| s.file_path) {
            Some(path) => os::show_in_folder(Path::new(&path)),
            None => os::show_in_folder(Path::new(&self.app.settings.read(cx).get().output_dir)),
        }
    }

    pub(crate) fn open_file(&self, id: DownloadId, cx: &App) {
        if let Some(path) = self.summary(id, cx).and_then(|s| s.file_path) {
            os::open_path(Path::new(&path));
        }
    }

    pub(crate) fn copy_title(&self, id: DownloadId, cx: &mut App) {
        if let Some(title) = self.summary(id, cx).and_then(|s| s.title) {
            cx.write_to_clipboard(ClipboardItem::new_string(title));
            toast(cx, Kind::Success, t!("dl-copied-title"), None);
        }
    }

    pub(crate) fn copy_link(&self, id: DownloadId, cx: &mut App) {
        if let Some(s) = self.summary(id, cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(s.url));
            toast(cx, Kind::Success, t!("dl-copied-link"), None);
        }
    }

    /// Hides the entry and removes it for good when the undo toast runs out.
    pub(crate) fn delete_entry(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.delete_entries(vec![id], cx);
    }

    /// Hides the entries that are not running and removes them for good when the one undo
    /// toast runs out.
    pub(crate) fn delete_entries(&mut self, ids: Vec<DownloadId>, cx: &mut Context<Self>) {
        let ids: Vec<DownloadId> = ids
            .into_iter()
            .filter(|id| self.summary(*id, cx).is_some_and(|s| !s.status.is_active()))
            .collect();
        if ids.is_empty() {
            return;
        }
        self.hidden.extend(&ids);
        if self.selected.is_some_and(|id| ids.contains(&id)) {
            self.selected = None;
        }
        let message = match ids.len() {
            1 => t!("dl-toast-deleted"),
            n => t!("dl-toast-deleted-many", count = n),
        };
        let undo = cx.entity();
        let back = ids.clone();
        toast(
            cx,
            Kind::Info,
            message,
            Some((
                t!("dl-undo"),
                Rc::new(move |cx: &mut App| {
                    undo.update(cx, |this, cx| {
                        for id in &back {
                            this.hidden.remove(id);
                        }
                        cx.notify();
                    })
                }),
            )),
        );
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO).await;
            this.update(cx, |this, cx| {
                for id in ids {
                    if this.hidden.remove(&id) {
                        this.app.downloads.update(cx, |d, cx| d.delete(id, cx));
                    }
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Asks before the file goes.
    pub(crate) fn ask_delete_file(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        if self.summary(id, cx).is_some_and(|s| s.file_path.is_some()) {
            self.confirm = Some(id);
            cx.notify();
        }
    }

    /// Deletes the file, then the entry.
    pub(crate) fn delete_file(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.delete_files(vec![id], cx);
    }

    /// Deletes each file, then its entry; an entry whose file cannot go stays.
    pub(crate) fn delete_files(&mut self, ids: Vec<DownloadId>, cx: &mut Context<Self>) {
        self.confirm = None;
        let mut failed = false;
        for id in ids {
            let Some(summary) = self.summary(id, cx) else {
                continue;
            };
            if let Some(path) = &summary.file_path {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        log::warn!("downloads: cannot delete {path}: {error}");
                        failed = true;
                        continue;
                    }
                }
            }
            if self.selected == Some(id) {
                self.selected = None;
            }
            self.app.downloads.update(cx, |d, cx| d.delete(id, cx));
        }
        if failed {
            toast(cx, Kind::Error, t!("dl-toast-delete-failed"), None);
        }
        cx.notify();
    }

    /// The ticked rows, in list order.
    pub(crate) fn ticked(&self, cx: &App) -> Vec<DownloadSummary> {
        self.ticks
            .ids(&self.order)
            .into_iter()
            .filter_map(|id| self.summary(id, cx))
            .collect()
    }

    /// Runs a selection-bar action on the ticked rows it fits; Remove and Delete files ask
    /// first. The ticks clear once it is done.
    pub(crate) fn bulk(&mut self, act: Bulk, cx: &mut Context<Self>) {
        let rows: Vec<DownloadSummary> = self
            .ticked(cx)
            .into_iter()
            .filter(|s| act.fits(s))
            .collect();
        if rows.is_empty() {
            return;
        }
        let ids: Vec<DownloadId> = rows.iter().map(|s| s.id).collect();
        match act {
            Bulk::Remove | Bulk::Delete => {
                self.bulk_confirm = Some((act == Bulk::Delete, ids));
                cx.notify();
                return;
            }
            Bulk::Copy => {
                let links: Vec<&str> = rows.iter().map(|s| s.url.as_str()).collect();
                cx.write_to_clipboard(ClipboardItem::new_string(links.join("\n")));
                toast(
                    cx,
                    Kind::Success,
                    t!("dl-copied-links", count = links.len()),
                    None,
                );
            }
            Bulk::Pause | Bulk::Resume | Bulk::Retry | Bulk::Cancel => {
                self.app.downloads.update(cx, |d, cx| {
                    for id in ids {
                        match act {
                            Bulk::Pause => d.pause(id, cx),
                            Bulk::Resume => d.resume(id, cx),
                            Bulk::Retry => d.retry(id, cx),
                            _ => d.cancel(id, cx),
                        }
                    }
                });
            }
        }
        self.ticks.clear();
        cx.notify();
    }

    /// The one confirmation of a bulk removal or file deletion went through.
    pub(crate) fn bulk_confirmed(&mut self, cx: &mut Context<Self>) {
        let Some((files, ids)) = self.bulk_confirm.take() else {
            return;
        };
        self.ticks.clear();
        match files {
            true => self.delete_files(ids, cx),
            false => self.delete_entries(ids, cx),
        }
    }

    /// Closes the detail panel.
    pub(crate) fn deselect(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        cx.notify();
    }

    /// Down and Up move the selection through the list.
    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        if self.order.is_empty() {
            return;
        }
        let at = self
            .selected
            .and_then(|id| self.order.iter().position(|x| *x == id));
        let next = match at {
            None => 0,
            Some(i) => i.saturating_add_signed(by).min(self.order.len() - 1),
        };
        self.select_at(next, cx);
    }

    fn select_at(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(id) = self.order.get(at) else { return };
        self.selected = Some(*id);
        // headers take lines too, so the list scrolls to the row's own line
        if let Some(line) = self.line_of(*id) {
            self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// Enter: selects the first row, or opens the finished file of the selected one.
    fn open_selected(&mut self, cx: &mut Context<Self>) {
        match self.selected {
            None => self.select_at(0, cx),
            Some(id) => self.open_file(id, cx),
        }
    }

    fn delete_selected(&mut self, with_file: bool, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        match with_file {
            true => self.ask_delete_file(id, cx),
            false => self.delete_entry(id, cx),
        }
    }

    /// Runs a keyboard action the Workspace forwards.
    pub(crate) fn key(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        match key {
            Key::Next => {
                self.step(1, cx);
                window.focus(&self.list_focus, cx);
            }
            Key::Previous => {
                self.step(-1, cx);
                window.focus(&self.list_focus, cx);
            }
            // with rows ticked, the keys act on those
            Key::TogglePause if !self.ticks.is_empty() => {
                let running = self.ticked(cx).iter().any(|s| Bulk::Pause.fits(s));
                self.bulk(if running { Bulk::Pause } else { Bulk::Resume }, cx);
            }
            Key::DeleteEntry if !self.ticks.is_empty() => self.bulk(Bulk::Remove, cx),
            Key::DeleteFile if !self.ticks.is_empty() => self.bulk(Bulk::Delete, cx),
            Key::Deselect if !self.ticks.is_empty() => {
                self.ticks.clear();
                cx.notify();
            }
            Key::TogglePause => self.toggle_pause(cx),
            Key::DeleteEntry => self.delete_selected(false, cx),
            Key::DeleteFile => self.delete_selected(true, cx),
            Key::Deselect => self.deselect(cx),
            Key::TickAll => {
                self.ticks.all(&self.order);
                cx.notify();
            }
            Key::TickNext | Key::TickPrevious => {
                let by = if key == Key::TickNext { 1 } else { -1 };
                if let Some(id) = self.ticks.extend(&self.order, self.selected, by) {
                    self.selected = Some(id);
                    if let Some(line) = self.line_of(id) {
                        self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
                    }
                }
                window.focus(&self.list_focus, cx);
                cx.notify();
            }
            Key::First => self.select_at(0, cx),
            Key::Last => self.select_at(self.order.len().saturating_sub(1), cx),
            Key::Open => self.open_selected(cx),
            Key::Paste => {
                if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                    let lines = one_per_line(&text);
                    let text = appended(self.intake.read(cx).text(), lines.lines());
                    self.intake.update(cx, |input, cx| input.set_text(text, cx));
                }
                self.intake.update(cx, |input, cx| input.focus(window, cx));
            }
            Key::FocusIntake => self.intake.update(cx, |input, cx| input.focus(window, cx)),
            Key::FocusSearch => self.search.update(cx, |input, cx| input.focus(window, cx)),
        }
    }

    fn download_event(
        &mut self,
        _: &Entity<state::Downloads>,
        event: &DownloadsEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = |this: &Self, id, cx: &App| {
            this.summary(id, cx)
                .map(|s| s.title.unwrap_or(s.url))
                .unwrap_or_default()
        };
        match event {
            DownloadsEvent::Completed(id) => {
                let id = *id;
                let title = name(self, id, cx);
                let this = cx.entity();
                toast(
                    cx,
                    Kind::Success,
                    t!("dl-toast-done", title = title.as_str()),
                    Some((
                        t!("dl-show-folder"),
                        Rc::new(move |cx: &mut App| {
                            this.update(cx, |this, cx| this.show_folder(id, cx))
                        }),
                    )),
                );
            }
            DownloadsEvent::Failed(id, error) => {
                let title = name(self, *id, cx);
                toast(
                    cx,
                    Kind::Error,
                    t!(
                        "dl-toast-failed",
                        title = title.as_str(),
                        reason = error_text(error.code).to_string()
                    ),
                    None,
                );
            }
            DownloadsEvent::Loaded => self.scene_rows(cx),
        }
    }

    // ---- list ----------------------------------------------------------------------------

    /// The lines of the list: the "In progress" group on top, the history below, both filtered.
    fn lines(&mut self, cx: &App) -> Vec<Line> {
        let filter = Filter::parse(self.search.read(cx).text());
        let rows = self.app.downloads.read(cx).rows();
        if self.known.len() != rows.len() {
            // the first load is not news; rows that come later fade in
            let news = !self.known.is_empty();
            for row in rows {
                if self.known.insert(row.id) && news {
                    self.fresh.insert(row.id, Instant::now());
                }
            }
        }
        self.fresh.retain(|_, at| at.elapsed() < FADE * 2);
        let mut progress = Vec::new();
        let mut history = Vec::new();
        for (at, row) in rows.iter().enumerate() {
            if self.hidden.contains(&row.id)
                || !self.chip.matches(row.status)
                || !filter.matches(row)
            {
                continue;
            }
            match row.status.is_terminal() {
                false => progress.push(at),
                true => history.push(at),
            }
        }
        // the queue reads oldest first
        progress.reverse();
        // what runs first, then the waiting and paused
        progress.sort_by_key(|at| !rows[*at].status.is_active());
        let mut lines = Vec::with_capacity(progress.len() + history.len() + 2);
        if !progress.is_empty() {
            lines.push(Line::Header("dl-group-progress", progress.len()));
            lines.extend(progress.into_iter().map(Line::Row));
        }
        if !history.is_empty() {
            lines.push(Line::Header("dl-group-history", history.len()));
            lines.extend(history.into_iter().map(Line::Row));
        }
        let store = self.app.downloads.read(cx);
        let hidden = &self.hidden;
        self.ticks
            .retain(|id| !hidden.contains(&id) && store.get(id).is_some());
        self.order.clear();
        self.order_lines.clear();
        for (n, line) in lines.iter().enumerate() {
            if let Line::Row(at) = line
                && let Some(row) = rows.get(*at)
            {
                self.order.push(row.id);
                self.order_lines.push(n);
            }
        }
        lines
    }

    /// The list line of a download.
    fn line_of(&self, id: DownloadId) -> Option<usize> {
        let at = self.order.iter().position(|x| *x == id)?;
        self.order_lines.get(at).copied()
    }

    fn chips(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let chips = [
            (StatusChip::All, "dl-chip-all"),
            (StatusChip::Active, "dl-chip-active"),
            (StatusChip::Done, "dl-chip-done"),
            (StatusChip::Failed, "dl-chip-failed"),
        ];
        let text = self.search.read(cx).text().to_owned();
        let tokens = text
            .split_whitespace()
            .filter(|word| {
                ["type:", "status:", "size:", "site:"]
                    .iter()
                    .any(|k| word.starts_with(k))
            })
            .map(|word| {
                let valid = !matches!(Filter::parse(word).terms.first(), Some(Term::Invalid(_)));
                (SharedString::from(word.to_owned()), valid)
            })
            .collect();
        let downloads = self.app.downloads.read(cx);
        // a narrow page puts the search on a line of its own rather than under the chips
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(theme.metrics.pad)
            .text_size(theme.text(Text::Label))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap(px(6.))
                    .children(chips.map(|(chip, key)| {
                        Chip::new(key, i18n::lookup(key, None))
                            .count(downloads.count(|s| chip.matches(s)))
                            .selected(self.chip == chip)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.chip = chip;
                                cx.notify();
                            }))
                    })),
            )
            .child(
                TokenInput::new(&self.search, tokens)
                    .spoken("dl-search-region", t!("dl-search-name")),
            )
            .into_any_element()
    }

    /// The status bar while rows are ticked: how many, and what can be done to them, each
    /// action saying how many rows it touches.
    fn selection_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let ticked = self.ticked(cx);
        let count = |act: Bulk| ticked.iter().filter(|s| act.fits(s)).count();
        let button = |id: &'static str, act: Bulk, icon: &'static str, label: SharedString| {
            Button::new(id)
                .ghost()
                .small()
                .icon(icon)
                .label(label)
                .disabled(count(act) == 0)
                .on_click(cx.listener(move |this, _, _, cx| this.bulk(act, cx)))
        };
        let all = self.ticks.has_all(&self.order);
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(4.))
            .min_h(theme.metrics.control)
            .px(theme.metrics.gutter)
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .whitespace_nowrap()
            .text_size(theme.text(Text::Body))
            .font_features(ui::tabular())
            .child(
                Checkbox::new("tick-every", all)
                    .name(t!("dl-tick-all"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ticks.all(&this.order);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .ml(px(8.))
                    .mr(px(8.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(t!("dl-bulk-count", count = ticked.len())),
            )
            .child(button(
                "bulk-pause",
                Bulk::Pause,
                "icons/pause.svg",
                t!("dl-bulk-pause", count = count(Bulk::Pause)),
            ))
            .child(button(
                "bulk-resume",
                Bulk::Resume,
                "icons/play.svg",
                t!("dl-bulk-resume", count = count(Bulk::Resume)),
            ))
            .child(button(
                "bulk-retry",
                Bulk::Retry,
                "icons/rotate-cw.svg",
                t!("dl-bulk-retry", count = count(Bulk::Retry)),
            ))
            .child(button(
                "bulk-cancel",
                Bulk::Cancel,
                "icons/x.svg",
                t!("dl-bulk-cancel", count = count(Bulk::Cancel)),
            ))
            .child(div().w(px(1.)).h(px(16.)).mx(px(4.)).bg(theme.border))
            .child(button(
                "bulk-copy",
                Bulk::Copy,
                "icons/link.svg",
                t!("dl-bulk-copy", count = count(Bulk::Copy)),
            ))
            .child(button(
                "bulk-remove",
                Bulk::Remove,
                "icons/trash-2.svg",
                t!("dl-bulk-remove", count = count(Bulk::Remove)),
            ))
            .child(
                button(
                    "bulk-delete",
                    Bulk::Delete,
                    "icons/trash-2.svg",
                    t!("dl-bulk-delete", count = count(Bulk::Delete)),
                )
                .text_color(theme.danger),
            )
            .child(div().flex_1())
            .child(
                Button::new("bulk-clear")
                    .ghost()
                    .small()
                    .icon("icons/x.svg")
                    .label(t!("dl-bulk-clear"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ticks.clear();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// The banner for tools that went missing after the first run.
    fn tools_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = *cx.theme();
        let tools = self.app.tools.read(cx);
        if !tools.detected() || tools.all_ready() {
            return None;
        }
        // an optional tool (aria2c) is never asked for
        let required: Vec<_> = tools.all().iter().filter(|s| !s.kind.optional()).collect();
        let missing: Vec<SharedString> = required
            .iter()
            .filter(|s| s.state != ToolState::Ready)
            .map(|s| crate::welcome::tool_name(s.kind))
            .collect();
        let busy = required
            .iter()
            .find(|s| matches!(s.state, ToolState::Downloading { .. }));
        let message = match busy {
            Some(tool) => {
                let percent = match tool.state {
                    ToolState::Downloading {
                        done,
                        total: Some(total),
                    } => (done as f64 / total.max(1) as f64 * 100.) as u32,
                    _ => 0,
                };
                t!(
                    "dl-notice-tools-busy",
                    tool = crate::welcome::tool_name(tool.kind).to_string(),
                    percent = percent
                )
            }
            None => t!("dl-notice-tools", tools = missing.join(", ")),
        };
        let failed = required
            .iter()
            .any(|s| matches!(s.state, ToolState::Failed(_)));
        let busy = busy.is_some();
        let tools_entity = self.app.tools.clone();
        Some(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .px(px(12.))
                .py(px(10.))
                .rounded(theme.radius)
                .border_1()
                .border_color(if failed { theme.danger } else { theme.border })
                .bg(theme.secondary)
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(10.))
                        .child(
                            gpui::svg()
                                .path(icons::path("icons/triangle-alert.svg"))
                                .size(px(16.))
                                .mt(px(1.))
                                .flex_none()
                                .text_color(if failed {
                                    theme.danger
                                } else {
                                    theme.progress_bar
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(t!("dl-notice-tools-title")),
                                )
                                .child(div().text_color(theme.muted_foreground).child(message)),
                        ),
                )
                .child(
                    Button::new("tools-fix")
                        .primary()
                        .disabled(busy)
                        .label(t!("dl-notice-tools-action"))
                        .on_click(move |_, _, cx| {
                            tools_entity.update(cx, |t, cx| t.ensure_missing(cx))
                        }),
                )
                .into_any_element(),
        )
    }

    fn status_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        if self.free_at.is_none_or(|at| at.elapsed() > FREE_EVERY) {
            let dir = self.app.settings.read(cx).get().output_dir.clone();
            self.free = os::free_space(Path::new(&dir));
            self.free_at = Some(Instant::now());
        }
        let downloads = self.app.downloads.read(cx);
        let running = downloads.count(|s| s.is_active());
        let paused = downloads.count(|s| s == DownloadStatus::Paused);
        let queued = downloads.count(|s| s == DownloadStatus::Queued);
        // one switch for the whole queue: pause while anything runs or waits, else resume
        let all = match (running + queued, paused) {
            (0, 0) => None,
            (0, _) => Some(("dl-resume-all", "icons/play.svg", true)),
            _ => Some(("dl-pause-all", "icons/pause.svg", false)),
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .whitespace_nowrap()
            .h(theme.metrics.control)
            .px(theme.metrics.gutter)
            .border_t_1()
            .border_color(theme.border)
            .text_size(theme.text(Text::Body))
            .text_color(theme.muted_foreground)
            .font_features(ui::tabular())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(match queued {
                        0 => t!("dl-status-bar", running = running, paused = paused),
                        q => t!(
                            "dl-status-bar-queued",
                            running = running,
                            paused = paused,
                            queued = q
                        ),
                    })
                    .children(all.map(|(key, icon, resume)| {
                        Button::new("queue-all")
                            .ghost()
                            .small()
                            .icon(icon)
                            .label(i18n::lookup(key, None))
                            .on_click(cx.listener(move |this, _, _, cx| this.all(resume, cx)))
                    })),
            )
            // notices live here, in the flow, so nothing is laid over the rows
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .px(px(16.))
                    .children(crate::toasts::inline(cx)),
            )
            .children(self.free.as_ref().map(|(free, drive)| {
                let size = format::bytes(*free as i64);
                t!(
                    "dl-free-space",
                    size = size.as_str(),
                    drive = drive.as_str()
                )
            }))
            .into_any_element()
    }

    /// The clipboard's links, offered right under the link field they would fill.
    fn clip_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let urls = self.clip_offer.clone()?;
        let theme = *cx.theme();
        let message = match urls.len() {
            1 => t!("dl-toast-clip-one", link = format::short_link(&urls[0], 48)),
            n => t!("dl-toast-clip-many", count = n),
        };
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(theme.metrics.field)
                .px(px(10.))
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.secondary)
                .text_size(theme.text(Text::Small))
                .child(
                    gpui::svg()
                        .path(icons::path("icons/info.svg"))
                        .size(px(14.))
                        .flex_none()
                        .text_color(theme.muted_foreground),
                )
                .child(div().flex_1().min_w_0().truncate().child(message))
                .child(
                    Button::new("clip-add")
                        .primary()
                        .small()
                        .label(t!("dl-toast-clip-add"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.clip_offer = None;
                            let urls = urls.clone();
                            this.app.intake.update(cx, |i, cx| i.add(&urls, cx));
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("clip-later")
                        .ghost()
                        .small()
                        .label(t!("dl-clip-not-now"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.clip_offer = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    fn intake_strip(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let text = self.intake.read(cx).text().trim().to_owned();
        let links = split_urls(&text).urls.len();
        let valid = links > 0;
        let invalid = !text.is_empty() && !valid;
        let note = invalid.then(|| t!("dl-intake-invalid"));
        // several links: the first shows, a chip counts the rest (owner pick 2A)
        let badge = (links > 1).then(|| t!("dl-intake-more", count = links - 1));
        self.intake.update(cx, |area, cx| area.set_badge(badge, cx));
        // the card has its own Download once it can download; one primary action at a time
        let own = matches!(
            self.card(cx),
            Card::Result(_) | Card::Playlist(_) | Card::Failed(_)
        );
        // the field keeps one line in the layout; the editor of several lies over the list
        let field = match self.editing {
            true => self.intake.read(cx).face(window, cx).into_any_element(),
            false => self.intake.clone().into_any_element(),
        };
        div()
            .flex()
            .items_start()
            .gap(theme.metrics.pad)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            if !this.editing && this.intake.read(cx).text().contains('\n') {
                                this.open_editor(window, cx);
                            }
                        }),
                    )
                    .child(field)
                    .when(self.editing, |this| this.child(self.editor(cx))),
            )
            .when_some(note, |this, note| {
                this.child(
                    div()
                        .flex_none()
                        .h(theme.metrics.field)
                        .flex()
                        .items_center()
                        .font_features(ui::tabular())
                        .text_size(theme.text(Text::Small))
                        .text_color(theme.muted_foreground)
                        .child(note),
                )
            })
            .when(!own, |this| {
                this.child(
                    // solid once there is a link to take, hollow and pencil until then
                    Button::new("intake-download")
                        .map(|button| match valid {
                            true => button.primary(),
                            false => button.outline(),
                        })
                        .large()
                        .icon("icons/download.svg")
                        .label(t!("dl-download"))
                        .disabled(!valid)
                        .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                )
            })
            .text_size(theme.text(Text::Body))
            .into_any_element()
    }

    /// Links dropped on the window: the text of dropped `.txt`/`.url` files is read for links.
    fn dropped(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        use std::io::Read as _;
        let mut urls: Vec<String> = Vec::new();
        for path in paths.paths() {
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if !["txt", "url", "list", "csv", "md"].contains(&ext.as_str()) {
                continue;
            }
            let mut text = String::new();
            // shortcut: the first MiB of a file is read, links further down are ignored
            let read = std::fs::File::open(path)
                .and_then(|file| file.take(1 << 20).read_to_string(&mut text));
            if let Err(error) = read {
                log::warn!(
                    "downloads: cannot read dropped file {}: {error}",
                    path.display()
                );
                continue;
            }
            for link in split_urls(&text.replace("URL=", " ")).urls {
                if !urls.contains(&link) {
                    urls.push(link);
                }
            }
        }
        match urls.is_empty() {
            true => toast(cx, Kind::Info, t!("dl-drop-none"), None),
            false => self.fill(&urls, cx),
        }
    }
}

/// The words for an error code: the shared `err-*` copy, else the short one of this page.
pub fn error_text(code: ErrorCode) -> SharedString {
    let key = code.message_key();
    let text = i18n::translate(&key);
    if text.as_ref() == key {
        return i18n::lookup(short_key(code.as_str()), None);
    }
    text
}

/// The same for a code read from a stored row.
pub fn error_text_of(code: &str) -> SharedString {
    match ErrorCode::from_str(code) {
        Ok(code) => error_text(code),
        Err(_) => i18n::lookup("dl-err-unknown", None),
    }
}

/// The Fluent key of a code's short cause, used until `err-*` ships.
fn short_key(code: &str) -> &'static str {
    match code {
        "login" | "age_restricted" | "cookies_locked" => "dl-err-login",
        "private" => "dl-err-private",
        "geo" => "dl-err-geo",
        "network" | "ssl" => "dl-err-network",
        "rate_limited" => "dl-err-rate",
        "disk_full" => "dl-err-disk",
        "permission" => "dl-err-permission",
        _ => "dl-err-unknown",
    }
}

impl Render for Downloads {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let (loaded, total) = {
            let d = self.app.downloads.read(cx);
            (d.loaded(), d.rows().len())
        };
        // an empty library invites a paste: the intake has focus
        if loaded && total == 0 && !std::mem::replace(&mut self.intake_offered, true) {
            self.intake.update(cx, |input, cx| input.focus(window, cx));
        }
        let keyboard = window.last_input_was_keyboard();
        let focus = crate::rows::ListFocus {
            ring: keyboard && self.list_focus.is_focused(window),
            within: keyboard && self.list_focus.contains_focused(window, cx),
        };
        let thumbs = self.app.settings.read(cx).get().show_thumbnails;
        let gutter = theme.metrics.gutter;
        // the page's width, measured as it was laid out (the window's until the first layout)
        let page = match self.page_width {
            w if w > Pixels::ZERO => w,
            _ => window.viewport_size().width,
        };
        let open = match self.ticks.is_empty() {
            true => self.selected.and_then(|id| self.summary(id, cx)),
            false => None,
        };
        // the details sit beside the list while both fit, else they take the list's place
        let side = px(crate::detail::PANEL) + px(1.);
        let fill =
            open.is_some() && f32::from(page - side) < crate::rows::Columns::least(gutter, thumbs);
        let list_width = match (&open, fill) {
            (Some(_), false) => page - side,
            _ => page,
        };
        let cols = crate::rows::Columns::for_width(list_width, gutter, thumbs);
        let lines = Rc::new(self.lines(cx));
        let filter_text = self.search.read(cx).text().to_owned();
        let list = match (lines.is_empty(), total == 0) {
            (true, _) if !loaded => div().into_any_element(),
            // the empty list is the drop target, so it is drawn as one: a dashed bounded zone
            (true, true) => div()
                .size_full()
                .p(theme.metrics.gutter)
                .child(
                    div()
                        .size_full()
                        .rounded(theme.radius)
                        .border_1()
                        .border_dashed()
                        .border_color(theme.muted_foreground.opacity(0.4))
                        .child(
                            Vacancy::new(t!("dl-empty"))
                                .icon("icons/download.svg")
                                .detail(t!("dl-empty-detail"))
                                .action(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .text_size(theme.text(Text::Small))
                                        .text_color(theme.muted_foreground)
                                        .child(
                                            Button::new("empty-paste")
                                                .primary()
                                                .icon("icons/clipboard-paste.svg")
                                                .label(t!("dl-empty-paste"))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.key(Key::Paste, window, cx)
                                                })),
                                        )
                                        .child(t!("dl-empty-keys"))
                                        .child(Kbd::new("secondary-v")),
                                )
                                .spoken("downloads-empty")
                                .size_full(),
                        ),
                )
                .into_any_element(),
            (true, false) => Vacancy::new(t!("dl-filtered-empty", filter = filter_text.as_str()))
                .icon("icons/search.svg")
                .action(
                    Button::new("clear-filter")
                        .outline()
                        .label(t!("dl-clear-filter"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.search.update(cx, |input, cx| input.set_text("", cx));
                            this.chip = StatusChip::All;
                            cx.notify();
                        })),
                )
                .spoken("downloads-filtered")
                .size_full()
                .into_any_element(),
            _ => {
                let items = lines.clone();
                Scroller::listing("downloads-scroll", &self.bar)
                    .fade_ends(px(16.))
                    .child(
                        uniform_list(
                            "downloads-rows",
                            lines.len(),
                            cx.processor(move |this, range: Range<usize>, _, cx| {
                                range
                                    .filter_map(|at| {
                                        items
                                            .get(at)
                                            .map(|line| this.line(*line, thumbs, focus, cols, cx))
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .size_full(),
                    )
                    .into_any_element()
            }
        };
        // with no row selected the list itself wears the focus ring; else the selected row does
        let list = div()
            .id("downloads-list")
            .relative()
            .size_full()
            .key_context(ROWS_CONTEXT)
            .track_focus(&self.list_focus)
            .child(list)
            .when(focus.ring && self.selected.is_none(), |this| {
                this.child(ui::focus_ring(cx))
            });
        // the card gives a list row back where the list drops its format column
        // the editor says what became of each link itself, and nothing under it may move
        let card = match self.editing {
            true => None,
            false => self.card_view(!cols.format, cx),
        };
        let banner = self.tools_banner(cx);
        let menu = self.row_menu(cx);
        let confirm = self.confirm_view(cx);
        let status = match self.ticks.is_empty() {
            true => self.status_bar(cx),
            false => self.selection_bar(cx),
        };
        let bulk_confirm = self.bulk_confirm_view(cx);

        let measured = cx.entity().downgrade();
        let clip = self.clip_strip(cx);
        div()
            .id("downloads-page")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.dropped(paths, cx)))
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(theme.table_active))
            // measures the page as laid out; a new width lays the rows out again
            .child(
                gpui::canvas(
                    move |bounds, _, cx| {
                        if let Some(page) = measured.upgrade() {
                            page.update(cx, |this, cx| {
                                if this.page_width != bounds.size.width {
                                    this.page_width = bounds.size.width;
                                    cx.notify();
                                }
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            // the link field, offer, card and filters keep the page's full width
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap(px(12.))
                    .px(gutter)
                    .pt(px(12.))
                    .pb(px(12.))
                    .child(
                        div()
                            .key_context(FORM_CONTEXT)
                            .on_action(cx.listener(|this, _: &Submit, _, cx| this.submit(cx)))
                            .child(self.intake_strip(window, cx)),
                    )
                    .children(clip)
                    .children(card)
                    .children(banner)
                    .child(self.chips(cx)),
            )
            // the list, and beside it (or in its place) the details
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(theme.border)
                    .when(!fill, |this| {
                        this.child(div().flex_1().min_w_0().h_full().child(list))
                    })
                    .children(open.map(|row| self.detail_panel(&row, fill, cx))),
            )
            .child(status)
            .children(menu)
            .children(confirm)
            .children(bulk_confirm)
            .children(self.formats_modal(cx))
    }
}

/// `current`'s lines, then each of `new` not already there, one to a line: links added to a
/// field that holds some join them instead of replacing them.
fn appended<'a>(current: &str, new: impl IntoIterator<Item = &'a str>) -> String {
    let mut lines: Vec<&str> = current
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    for line in new.into_iter().map(str::trim) {
        if !line.is_empty() && !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines.join(
        "
",
    )
}

/// Every whitespace-separated word of `text` on a line of its own.
/// Every whitespace-separated word of a line that holds a link on a line of its own; a line
/// without one stays whole, so a stray sentence is one skipped line.
fn one_per_line(text: &str) -> String {
    text.lines()
        .flat_map(|line| match split_urls(line).urls.is_empty() {
            true => vec![line.trim()],
            false => line.split_whitespace().collect(),
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn added_links_join_the_field_without_repeats() {
        assert_eq!(
            super::appended(
                "https://a.example/1
",
                ["https://b.example/2", "https://a.example/1"]
            ),
            "https://a.example/1
https://b.example/2"
        );
        assert_eq!(
            super::appended("", ["https://a.example/1"]),
            "https://a.example/1"
        );
    }

    #[test]
    fn a_paste_splits_links_and_keeps_a_stray_line_whole() {
        assert_eq!(
            super::one_per_line("https://a.example/1 https://b.example/2\r\n\nnot a link  \n"),
            "https://a.example/1\nhttps://b.example/2\nnot a link"
        );
    }
}
