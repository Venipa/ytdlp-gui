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
    AnyElement, App, ClipboardItem, Context, Entity, ExternalPaths, FocusHandle, Pixels, Point,
    ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, Window, div, px,
    uniform_list,
};
use i18n::t;
use state::{AppState, DownloadsEvent, IntakeEvent, ProbeState};
use ui::{
    ActiveTheme as _, Button, Chip, FORM_CONTEXT, Input, MONO, Scrollbar, Scroller, Submit, Text,
    TokenInput, Vacancy,
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
/// The narrowest window that docks the detail panel beside the list; narrower ones lay it over.
const DOCK_AT: f32 = 1100.;

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
#[derive(Clone, Copy)]
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
}

/// The Downloads page.
pub struct Downloads {
    pub(crate) app: AppState,
    pub(crate) intake: Entity<Input>,
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
    /// Entries hidden while their undo toast is up.
    pub(crate) hidden: HashSet<DownloadId>,
    /// The ids in list order, for the arrow keys.
    order: Vec<DownloadId>,
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
            Input::new(t!("dl-intake-hint"), cx)
                .name("dl-intake-name")
                .icon("icons/link.svg")
                .large()
                .clearable()
        });
        let search = cx.new(|cx| {
            Input::new(t!("dl-search-hint"), cx)
                .name("dl-search-name")
                .icon("icons/search.svg")
                .clearable()
        });
        let subs = vec![
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe(&intake, |this, _, cx| this.intake_changed(cx)),
            cx.observe(&app.downloads, |_, _, cx| cx.notify()),
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
            list_focus: cx.focus_handle().tab_stop(true),
            detail_bar,
            log_bar,
            details_open: true,
            intake_offered: false,
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
            last_text: String::new(),
            formats: None,
            format_scroll: Default::default(),
            playlist_off: HashSet::new(),
            menu: None,
            confirm: None,
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

    /// Puts links into the field and starts reading them.
    pub(crate) fn fill(&mut self, urls: &[String], cx: &mut Context<Self>) {
        let text = urls.join(" ");
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
                let urls = urls.clone();
                let intake = self.app.intake.clone();
                let message = match urls.len() {
                    1 => t!("dl-toast-clip-one", link = format::short_link(&urls[0], 36)),
                    n => t!("dl-toast-clip-many", count = n),
                };
                toast(
                    cx,
                    Kind::Info,
                    message,
                    Some((
                        t!("dl-toast-clip-add"),
                        Rc::new(move |cx: &mut App| {
                            let urls = urls.clone();
                            intake.update(cx, |i, cx| i.add(&urls, cx));
                        }),
                    )),
                );
            }
        }
    }

    /// Typing edits a link, pasting starts reading it.
    fn intake_changed(&mut self, cx: &mut Context<Self>) {
        let text = self.intake.read(cx).text().to_owned();
        if text == self.last_text {
            return;
        }
        let pasted = text.len() > self.last_text.len() + 1;
        self.last_text.clone_from(&text);
        if text.trim().is_empty() {
            self.clear_card(cx);
        } else if pasted {
            self.begin(cx);
        } else if !self.urls.is_empty() {
            self.clear_card(cx);
        }
        cx.notify();
    }

    fn probe_opts(&self, cx: &App) -> ProbeOpts {
        let settings = self.app.settings.read(cx).get();
        ProbeOpts {
            timeout_s: None,
            flat_playlist: true,
            cookies: Some(settings.cookies.clone()),
            proxy: settings.proxy.clone(),
        }
    }

    /// Reads the field: every link is probed and the card follows the results.
    pub(crate) fn begin(&mut self, cx: &mut Context<Self>) {
        let text = self.intake.read(cx).text().to_owned();
        let split = split_urls(&text);
        self.release(cx);
        self.skipped = split.invalid.into_iter().map(|(_, word)| word).collect();
        self.urls = split.urls;
        self.playlist_off.clear();
        self.remembered = Preset::from_str(&self.app.settings.read(cx).get().last_preset)
            .unwrap_or(Preset::DEFAULT);
        self.preset = self.remembered;
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
        if self.enter_waits && matches!(self.card(cx), Card::Result(_)) {
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
        match self.card(cx) {
            Card::None | Card::NotLink => self.begin(cx),
            Card::Probing => self.enter_waits = true,
            Card::Failed(_) => {
                // read the page again
                let urls = self.urls.clone();
                self.fill(&urls, cx);
            }
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
                    self.enqueue(&p.url, preset, "", None, Some(p), cx);
                }
                ready.len()
            }
            Card::Playlist(p) => {
                let page = p.webpage_url.clone().unwrap_or_else(|| p.url.clone());
                let entries = p.entries.clone().unwrap_or_default();
                let mut count = 0;
                let off = self.playlist_off.clone();
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
        if preset != Preset::Custom {
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
        let spec = self
            .app
            .job_spec(url, preset, format_spec, playlist_index, cx);
        let label = match preset {
            Preset::Custom => self.formats_label(probe, format_spec),
            other => format::preset_label(other),
        };
        let task = self
            .app
            .downloads
            .update(cx, |d, cx| d.enqueue(spec, probe, Some(label), cx));
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
        }
    }

    pub(crate) fn copy_link(&self, id: DownloadId, cx: &mut App) {
        if let Some(s) = self.summary(id, cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(s.url));
        }
    }

    /// Hides the entry and removes it for good when the undo toast runs out.
    pub(crate) fn delete_entry(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        match self.summary(id, cx) {
            Some(s) if !s.status.is_active() => {}
            _ => return,
        }
        self.hidden.insert(id);
        if self.selected == Some(id) {
            self.selected = None;
        }
        let undo = cx.entity();
        toast(
            cx,
            Kind::Info,
            t!("dl-toast-deleted"),
            Some((
                t!("dl-undo"),
                Rc::new(move |cx: &mut App| {
                    undo.update(cx, |this, cx| {
                        this.hidden.remove(&id);
                        cx.notify();
                    })
                }),
            )),
        );
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO).await;
            this.update(cx, |this, cx| {
                if this.hidden.remove(&id) {
                    this.app.downloads.update(cx, |d, cx| d.delete(id, cx));
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
        self.confirm = None;
        let Some(summary) = self.summary(id, cx) else {
            return;
        };
        if let Some(path) = &summary.file_path {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    log::warn!("downloads: cannot delete {path}: {error}");
                    toast(cx, Kind::Error, t!("dl-toast-delete-failed"), None);
                    cx.notify();
                    return;
                }
            }
        }
        if self.selected == Some(id) {
            self.selected = None;
        }
        self.app.downloads.update(cx, |d, cx| d.delete(id, cx));
        cx.notify();
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
            Key::TogglePause => self.toggle_pause(cx),
            Key::DeleteEntry => self.delete_selected(false, cx),
            Key::DeleteFile => self.delete_selected(true, cx),
            Key::Deselect => self.deselect(cx),
            Key::First => self.select_at(0, cx),
            Key::Last => self.select_at(self.order.len().saturating_sub(1), cx),
            Key::Open => self.open_selected(cx),
            Key::Paste => {
                if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                    self.intake
                        .update(cx, |input, cx| input.set_text(ui::single_line(&text), cx));
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
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(theme.metrics.pad)
            .text_size(theme.text(Text::Label))
            .child(div().flex().gap(px(6.)).children(chips.map(|(chip, key)| {
                Chip::new(key, i18n::lookup(key, None))
                    .count(downloads.count(|s| chip.matches(s)))
                    .selected(self.chip == chip)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.chip = chip;
                        cx.notify();
                    }))
            })))
            .child(
                TokenInput::new(&self.search, tokens)
                    .spoken("dl-search-region", t!("dl-search-name")),
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
        let missing: Vec<SharedString> = tools
            .all()
            .iter()
            .filter(|s| s.state != ToolState::Ready)
            .map(|s| crate::welcome::tool_name(s.kind))
            .collect();
        let busy = tools
            .all()
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
        let failed = tools
            .all()
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
        div()
            .flex()
            .flex_none()
            .justify_between()
            .items_center()
            .h(theme.metrics.control)
            .px(theme.metrics.gutter)
            .border_t_1()
            .border_color(theme.border)
            .text_size(theme.text(Text::Small))
            .text_color(theme.muted_foreground)
            .font_features(ui::tabular())
            .child(match queued {
                0 => t!("dl-status-bar", running = running, paused = paused),
                q => t!(
                    "dl-status-bar-queued",
                    running = running,
                    paused = paused,
                    queued = q
                ),
            })
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

    fn intake_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let text = self.intake.read(cx).text().trim().to_owned();
        let valid = !split_urls(&text).urls.is_empty();
        let invalid = !text.is_empty() && !valid;
        // the card has its own Download once it can download; one primary action at a time
        let own = matches!(
            self.card(cx),
            Card::Result(_) | Card::Batch | Card::Playlist(_) | Card::Failed(_)
        );
        div()
            .flex()
            .items_center()
            .gap(theme.metrics.pad)
            .h(theme.metrics.field)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_family(MONO)
                    .child(self.intake.clone()),
            )
            .when(invalid, |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(theme.text(Text::Small))
                        .text_color(theme.muted_foreground)
                        .child(t!("dl-intake-invalid")),
                )
            })
            .when(!own, |this| {
                this.child(
                    Button::new("intake-download")
                        .outline()
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
        let ring = self.list_focus.is_focused(window) && window.last_input_was_keyboard();
        // a wide window docks the detail panel beside the list, a narrow one lays it over the list
        let width = window.viewport_size().width;
        let docked = width >= px(DOCK_AT);
        let list_width = match (self.selected.is_some(), docked) {
            (true, true) => width - px(crate::detail::PANEL),
            _ => width,
        };
        let cols = crate::rows::Columns::for_width(list_width);
        let lines = Rc::new(self.lines(cx));
        let filter_text = self.search.read(cx).text().to_owned();
        let thumbs = self.app.settings.read(cx).get().show_thumbnails;
        let list = match (lines.is_empty(), total == 0) {
            (true, _) if !loaded => div().into_any_element(),
            (true, true) => Vacancy::new(t!("dl-empty"))
                .icon("icons/download.svg")
                .detail(t!("dl-empty-detail"))
                .action(
                    Button::new("empty-paste")
                        .outline()
                        .icon("icons/clipboard-paste.svg")
                        .label(t!("dl-empty-paste"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.key(Key::Paste, window, cx)),
                        ),
                )
                .spoken("downloads-empty")
                .size_full()
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
                                            .map(|line| this.line(*line, thumbs, ring, cols, cx))
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
        let list = div()
            .id("downloads-list")
            .size_full()
            .key_context(ROWS_CONTEXT)
            .track_focus(&self.list_focus)
            .child(list);
        let selected = self.selected.and_then(|id| self.summary(id, cx));
        let card = self.card_view(cx);
        let banner = self.tools_banner(cx);
        let menu = self.row_menu(cx);
        let confirm = self.confirm_view(cx);
        let status = self.status_bar(cx);

        div()
            .id("downloads-page")
            .size_full()
            .flex()
            .flex_col()
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.dropped(paths, cx)))
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(theme.table_active))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_none()
                                    .gap(px(12.))
                                    .px(theme.metrics.gutter)
                                    .pt(px(12.))
                                    .pb(px(12.))
                                    .child(
                                        div()
                                            .key_context(FORM_CONTEXT)
                                            .on_action(cx.listener(|this, _: &Submit, _, cx| {
                                                this.submit(cx)
                                            }))
                                            .child(self.intake_strip(cx)),
                                    )
                                    .children(card)
                                    .children(banner)
                                    .child(self.chips(cx)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .border_t_1()
                                    .border_color(theme.border)
                                    .child(list),
                            ),
                    )
                    .children(selected.map(|row| self.detail_panel(&row, !docked, cx))),
            )
            .child(status)
            .children(menu)
            .children(confirm)
            .children(self.formats_modal(cx))
    }
}
