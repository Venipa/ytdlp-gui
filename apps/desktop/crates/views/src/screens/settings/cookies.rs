//! The Cookies page: every domain of every cookie set with its own switch, the sets in priority
//! order (the higher set wins a site), adding a file, a paste or a browser copy, and a cookies.txt
//! editor. Downloads get only their site's cookies (`state::CookieFiles::for_run`).

use std::collections::{HashMap, HashSet};

use domain::cookies::{DomainSummary, check, site_of, summarize};
use domain::{CookieSet, CookieSource, Settings};
use gpui::accesskit::Role;
use gpui::prelude::*;
use gpui::{AnyElement, Context, ElementId, Entity, SharedString, Window, div, px, svg};
use i18n::t;
use state::Hooks;
use state::cookies::browser_name;
use ui::{
    ActiveTheme as _, Button, Chip, Input, MONO, Notice, Scrollbar, Scroller, Switch, Text,
    TextArea, tabular,
};

use super::SettingsView;
use super::layout::{row, section, stacked};
use crate::format;

/// Browsers offered for a copy on this system (yt-dlp keys).
const BROWSERS: &[&str] = if cfg!(target_os = "macos") {
    &["firefox", "safari", "chrome", "edge", "brave"]
} else if cfg!(windows) {
    &["firefox", "chrome", "edge", "brave"]
} else {
    &["firefox", "chrome", "chromium", "brave"]
};

/// State the Cookies page keeps between frames.
pub(super) struct CookiePage {
    /// Domains of each set by id; `None` while the set has no text (a browser never read).
    domains: HashMap<String, Option<Vec<DomainSummary>>>,
    /// The (id, updated) pairs `domains` was read for.
    read_for: Vec<(String, i64)>,
    search: Entity<Input>,
    editor: Option<Editor>,
    /// The browser being read right now.
    reading: Option<String>,
    /// The add/read rows are shown (closed until the row is clicked).
    add_open: bool,
    tab: Tab,
    /// Sites unfolded to their domains.
    open_sites: HashSet<String>,
    sites_bar: Entity<Scrollbar>,
}

/// The two tabs of the page (round 3, 1B).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Sets,
    Sites,
}

/// How tall the site list is; it scrolls inside (about 11 lines).
const SITES_HEIGHT: f32 = 400.;

struct Editor {
    id: String,
    area: Entity<TextArea>,
    original: String,
    /// The check of the text as it stands: the count, or what is wrong.
    status: Result<SharedString, SharedString>,
}

impl CookiePage {
    pub(super) fn new(cx: &mut Context<SettingsView>) -> Self {
        let search = cx.new(|cx| {
            Input::new("set-ck-search-hint", cx)
                .icon("icons/search.svg")
                .clearable()
        });
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        Self {
            domains: HashMap::new(),
            read_for: vec![],
            search,
            editor: None,
            reading: None,
            add_open: false,
            tab: Tab::Sets,
            open_sites: HashSet::new(),
            sites_bar: cx
                .new(|cx| Scrollbar::new(gpui::ScrollHandle::new()).watching(cx.entity_id())),
        }
    }
}

fn key_of(sets: &[CookieSet]) -> Vec<(String, i64)> {
    sets.iter().map(|s| (s.id.clone(), s.updated_ms)).collect()
}

fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// "Line 3: needs 7 fields …", or the count.
fn checked(text: &str) -> Result<SharedString, SharedString> {
    match check(text) {
        Ok(count) => Ok(t!("set-ck-ok", count = count)),
        Err((0, why)) => Err(i18n::lookup(why, None)),
        Err((line, why)) => Err(t!(
            "set-ck-bad-line",
            line = line,
            why = i18n::lookup(why, None).to_string()
        )),
    }
}

impl SettingsView {
    /// Reads every set's domains off the UI thread.
    pub(super) fn reload_cookie_domains(&mut self, cx: &mut Context<Self>) {
        let sets = self.settings().read(cx).get().cookies.sets.clone();
        let key = key_of(&sets);
        let files = self.state.cookies.clone();
        let io = self.state.io.clone();
        cx.spawn(async move |this, cx| {
            let read = io
                .spawn_blocking(move || {
                    sets.into_iter()
                        .map(|s| {
                            let domains = files.read(&s.id).map(|text| summarize(&text));
                            (s.id, domains)
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await;
            if let Ok(domains) = read {
                this.update(cx, |this, cx| {
                    this.cookie_page.domains = domains;
                    this.cookie_page.read_for = key;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// The sets changed (added, removed, re-read, or edited in settings.json): read again.
    pub(super) fn cookies_changed(&mut self, current: &Settings, cx: &mut Context<Self>) {
        if key_of(&current.cookies.sets) != self.cookie_page.read_for {
            self.reload_cookie_domains(cx);
        }
    }

    /// Domains that switched-on sets give cookies for.
    pub(super) fn cookie_domain_count(&self, s: &Settings) -> usize {
        let mut seen: Vec<&str> = vec![];
        for set in s.cookies.sets.iter().filter(|set| set.on) {
            if let Some(Some(domains)) = self.cookie_page.domains.get(&set.id) {
                for d in domains {
                    if !set.off_domains.contains(&d.domain) && !seen.contains(&d.domain.as_str()) {
                        seen.push(&d.domain);
                    }
                }
            }
        }
        seen.len()
    }

    fn edit_sets(&self, edit: impl FnOnce(&mut Vec<CookieSet>), cx: &mut Context<Self>) {
        self.settings()
            .update(cx, |s, cx| s.update(|st| edit(&mut st.cookies.sets), cx));
    }

    fn cookie_note(&mut self, note: SharedString, failed: bool, cx: &mut Context<Self>) {
        self.cookie_note = Some((note, failed));
        cx.notify();
    }

    /// Stores `text` as a new set on top of the list (it wins over older sets).
    fn add_set(&mut self, source: CookieSource, name: String, text: &str, cx: &mut Context<Self>) {
        if let Err(why) = checked(text) {
            return self.cookie_note(why, true, cx);
        }
        let files = self.state.cookies.clone();
        let now = now_ms();
        let id = files.new_id(now);
        if files.write(&id, text).is_err() {
            return self.cookie_note(t!("set-cookies-save-failed"), true, cx);
        }
        let note = t!("set-ck-added", set = &name);
        self.edit_sets(
            |sets| {
                sets.insert(
                    0,
                    CookieSet {
                        id,
                        name,
                        source,
                        updated_ms: now,
                        on: true,
                        off_domains: vec![],
                    },
                )
            },
            cx,
        );
        self.cookie_note(note, false, cx);
    }

    fn import_cookie_file(&mut self, cx: &mut Context<Self>) {
        self.choose_file(
            |this, path, cx| match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "cookies.txt".into());
                    this.add_set(CookieSource::File, name, &text, cx);
                }
                Err(_) => this.cookie_note(t!("set-cookies-bad"), true, cx),
            },
            cx,
        );
    }

    fn paste_cookies(&mut self, cx: &mut Context<Self>) {
        let text = cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .unwrap_or_default();
        let (time, date) = format::when(now_ms());
        let name = t!("set-ck-pasted-name", date = date.unwrap_or(time)).to_string();
        self.add_set(CookieSource::Paste, name, &text, cx);
    }

    /// Copies `browser`'s cookies into a new set, or into the set `into` (Refresh).
    fn read_browser(&mut self, browser: String, into: Option<String>, cx: &mut Context<Self>) {
        let Some(read) = cx
            .try_global::<Hooks>()
            .and_then(|h| h.browser_cookies.clone())
        else {
            return;
        };
        self.cookie_page.reading = Some(browser.clone());
        cx.notify();
        let io = self.state.io.clone();
        cx.spawn(async move |this, cx| {
            let key = browser.clone();
            let result = io
                .spawn_blocking(move || read(&key))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            this.update(cx, |this, cx| {
                this.cookie_page.reading = None;
                let name = browser_name(&browser);
                match (result, into) {
                    (Ok(text), Some(id)) => {
                        if this.state.cookies.write(&id, &text).is_err() {
                            return this.cookie_note(t!("set-cookies-save-failed"), true, cx);
                        }
                        let now = now_ms();
                        this.edit_sets(
                            |sets| {
                                if let Some(set) = sets.iter_mut().find(|s| s.id == id) {
                                    set.updated_ms = now;
                                }
                            },
                            cx,
                        );
                        this.cookie_note(t!("set-cookies-saved"), false, cx);
                    }
                    (Ok(text), None) => {
                        this.add_set(CookieSource::Browser(browser.clone()), name, &text, cx)
                    }
                    (Err(error), _) => this.cookie_note(
                        t!("set-ck-read-failed", browser = &name, error = &error),
                        true,
                        cx,
                    ),
                }
            })
            .ok();
        })
        .detach();
    }

    fn remove_set(&mut self, id: &str, cx: &mut Context<Self>) {
        self.state.cookies.remove(id);
        let id = id.to_owned();
        self.edit_sets(|sets| sets.retain(|s| s.id != id), cx);
    }

    fn move_set(&mut self, at: usize, up: bool, cx: &mut Context<Self>) {
        self.edit_sets(
            |sets| {
                let to = if up { at.checked_sub(1) } else { Some(at + 1) };
                if let Some(to) = to.filter(|to| *to < sets.len()) {
                    sets.swap(at, to);
                }
            },
            cx,
        );
    }

    fn open_editor(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.state.cookies.read(id).unwrap_or_default();
        let area = cx.new(|cx| {
            let mut area = TextArea::new("set-ck-edit-desc", cx)
                .name("set-ck-edit-desc")
                .max_lines(18.);
            area.set_text(text.clone(), cx);
            area
        });
        cx.observe(&area, |this, area, cx| {
            let status = checked(area.read(cx).text());
            if let Some(editor) = &mut this.cookie_page.editor {
                editor.status = status;
            }
            cx.notify();
        })
        .detach();
        let focus = area.clone();
        focus.update(cx, |area, cx| area.focus(window, cx));
        self.cookie_page.editor = Some(Editor {
            id: id.to_owned(),
            area,
            status: checked(&text),
            original: text,
        });
        cx.notify();
    }

    fn save_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.cookie_page.editor else {
            return;
        };
        let text = editor.area.read(cx).text().to_owned();
        if checked(&text).is_err() {
            return;
        }
        let id = editor.id.clone();
        if self.state.cookies.write(&id, &text).is_err() {
            return self.cookie_note(t!("set-cookies-save-failed"), true, cx);
        }
        let now = now_ms();
        self.edit_sets(
            |sets| {
                if let Some(set) = sets.iter_mut().find(|s| s.id == id) {
                    set.updated_ms = now;
                }
            },
            cx,
        );
        self.cookie_page.editor = None;
        self.cookie_note(t!("set-cookies-saved"), false, cx);
    }

    pub(super) fn page_cookies(
        &mut self,
        s: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if let Some(editor) = &self.cookie_page.editor {
            let name = s
                .cookies
                .sets
                .iter()
                .find(|set| set.id == editor.id)
                .map(|set| set.name.clone())
                .unwrap_or_default();
            return vec![self.editor_section(editor, &name, cx)];
        }
        let mut page = vec![self.cookie_tabs(s, cx)];
        match self.cookie_page.tab {
            Tab::Sets => {
                page.push(self.add_section(window, cx));
                page.push(self.sets_section(s, cx));
            }
            Tab::Sites => page.push(self.sites_section(s, cx)),
        }
        page
    }

    fn add_section(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let open = self.cookie_page.add_open;
        // a disclosure row in the settings-row style: the whole row opens the add/read rows under it
        let toggle_id = ElementId::from("ck-add-toggle");
        let (hover, press) = (theme.secondary_hover, theme.secondary_active);
        let head = ui::focusable(div().id(toggle_id.clone()), &toggle_id, window, cx)
            .role(Role::Button)
            .aria_label(t!("set-ck-add"))
            .aria_description(t!("set-ck-add-desc"))
            .aria_expanded(open)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .mx(px(-12.))
            .px(px(12.))
            .py(px(10.))
            .min_h(px(52.))
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .active(move |style| style.bg(press))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(t!("set-ck-add"))
                    .child(
                        div()
                            .text_size(theme.text(Text::Small))
                            .text_color(theme.muted_foreground)
                            .child(t!("set-ck-add-desc")),
                    ),
            )
            .child(
                svg()
                    .path(icons::path(if open {
                        "icons/chevron-up.svg"
                    } else {
                        "icons/chevron-down.svg"
                    }))
                    .size(px(16.))
                    .flex_none()
                    .text_color(theme.muted_foreground),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.cookie_page.add_open = !open;
                cx.notify();
            }));
        let reading = self.cookie_page.reading.clone();
        let buttons = div()
            .flex()
            .gap(px(8.))
            .child(
                Button::new("ck-import")
                    .outline()
                    .icon("icons/folder-plus.svg")
                    .label(t!("set-ck-import"))
                    .on_click(cx.listener(|this, _, _, cx| this.import_cookie_file(cx))),
            )
            .child(
                Button::new("ck-paste")
                    .outline()
                    .label(t!("set-ck-paste"))
                    .on_click(cx.listener(|this, _, _, cx| this.paste_cookies(cx))),
            );
        let browsers = div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .children(BROWSERS.iter().map(|&b| {
                let busy = reading.as_deref() == Some(b);
                Button::new(SharedString::from(format!("ck-read-{b}")))
                    .outline()
                    .label(if busy {
                        t!("set-ck-reading", browser = browser_name(b))
                    } else {
                        SharedString::from(browser_name(b))
                    })
                    .disabled(reading.is_some())
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.read_browser(b.into(), None, cx)),
                    )
            }));
        // only where that browser on this system has trouble (round 1, 6A)
        let note = if cfg!(target_os = "macos") {
            Some(t!("set-ck-note-safari"))
        } else if cfg!(windows) {
            Some(t!("set-cookies-chrome"))
        } else {
            None
        };
        let mut rows = vec![head.into_any_element()];
        if open {
            rows.push(stacked(t!("set-ck-import-title"), None, buttons, cx));
            rows.push(stacked(
                t!("set-ck-browser"),
                Some(t!("set-ck-browser-desc")),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(browsers)
                    .children(
                        note.map(|note| Notice::new(t!("set-ck-browser"), note).max_w_full()),
                    ),
                cx,
            ));
        }
        // the outcome of the last add or read shows also while the section is closed
        if let Some((note, failed)) = self.cookie_note.clone() {
            rows.push(
                div()
                    .py(px(10.))
                    .text_size(theme.text(Text::Small))
                    .text_color(if failed {
                        theme.danger
                    } else {
                        theme.muted_foreground
                    })
                    .child(note)
                    .into_any_element(),
            );
        }
        section(None, rows, cx)
    }

    /// The Sets / Sites tabs over the page.
    fn cookie_tabs(&self, s: &Settings, cx: &mut Context<Self>) -> AnyElement {
        let sites = self.cookie_sites(s, "").len();
        let tab = self.cookie_page.tab;
        div()
            .flex()
            .gap(px(4.))
            .child(
                Chip::new(
                    "ck-tab-sets",
                    t!("set-ck-tab-sets", count = s.cookies.sets.len()),
                )
                .selected(tab == Tab::Sets)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cookie_page.tab = Tab::Sets;
                    cx.notify();
                })),
            )
            .child(
                Chip::new("ck-tab-sites", t!("set-ck-tab-sites", count = sites))
                    .selected(tab == Tab::Sites)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cookie_page.tab = Tab::Sites;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Every (set, domain) line grouped by site, sites sorted by name, lines by domain then set
    /// priority. A non-empty `query` keeps sites whose name or a domain contains it.
    fn cookie_sites<'a>(
        &'a self,
        s: &'a Settings,
        query: &str,
    ) -> Vec<(String, Vec<(usize, &'a DomainSummary)>)> {
        let mut sites: std::collections::BTreeMap<String, Vec<(usize, &DomainSummary)>> =
            Default::default();
        for (i, set) in s.cookies.sets.iter().enumerate() {
            if let Some(Some(domains)) = self.cookie_page.domains.get(&set.id) {
                for d in domains {
                    sites.entry(site_of(&d.domain)).or_default().push((i, d));
                }
            }
        }
        sites
            .into_iter()
            .filter(|(site, lines)| {
                query.is_empty()
                    || site.contains(query)
                    || lines.iter().any(|(_, d)| d.domain.contains(query))
            })
            .map(|(site, mut lines)| {
                lines.sort_by(|a, b| a.1.domain.cmp(&b.1.domain).then(a.0.cmp(&b.0)));
                (site, lines)
            })
            .collect()
    }

    fn sites_section(&self, s: &Settings, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let sets = &s.cookies.sets;
        let query = self
            .cookie_page
            .search
            .read(cx)
            .text()
            .trim()
            .to_lowercase();
        let sites = self.cookie_sites(s, &query);
        let header = |label: SharedString| {
            div()
                .text_size(theme.text(Text::Small))
                .text_color(theme.muted_foreground)
                .child(label)
        };
        let mut lines: Vec<AnyElement> = vec![];
        for (site, members) in &sites {
            let single = members.len() == 1 && members[0].1.domain == *site;
            if single {
                let (i, d) = members[0];
                lines.push(self.domain_line(sets, i, d, false, cx));
                continue;
            }
            let open = !query.is_empty() || self.cookie_page.open_sites.contains(site);
            lines.push(self.site_line(sets, site, members, open, cx));
            if open {
                for &(i, d) in members {
                    lines.push(self.domain_line(sets, i, d, true, cx));
                }
            }
        }
        if lines.is_empty() {
            lines.push(
                div()
                    .py(px(12.))
                    .text_color(theme.muted_foreground)
                    .child(t!("set-ck-domains-empty"))
                    .into_any_element(),
            );
        }
        let rows = vec![
            div()
                .py(px(10.))
                .child(self.cookie_page.search.clone())
                .into_any_element(),
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .h(px(28.))
                .child(header(t!("set-ck-col-site")).flex_1())
                .child(header(t!("set-ck-col-from")).w(px(140.)))
                .child(
                    header(t!("set-ck-col-count"))
                        .w(px(56.))
                        .flex()
                        .justify_end(),
                )
                .child(header(t!("set-ck-col-expires")).w(px(92.)))
                .child(div().w(px(32.)))
                .into_any_element(),
            // a fixed height: the page keeps its length whatever the cookie count (round 3, 2A)
            div()
                .h(px(SITES_HEIGHT))
                .mx(px(-12.))
                .child(
                    Scroller::new("ck-sites", &self.cookie_page.sites_bar)
                        .fade_ends(px(12.))
                        .child(div().px(px(12.)).flex().flex_col().children(lines)),
                )
                .into_any_element(),
        ];
        section(None, rows, cx)
    }

    /// A site with several lines: caret, name, the set used, its cookie count, and one switch
    /// for all of its lines.
    fn site_line(
        &self,
        sets: &[CookieSet],
        site: &str,
        members: &[(usize, &DomainSummary)],
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let enabled =
            |i: usize, d: &DomainSummary| sets[i].on && !sets[i].off_domains.contains(&d.domain);
        let any_on = members.iter().any(|&(i, d)| enabled(i, d));
        let used = members
            .iter()
            .find(|&&(i, d)| enabled(i, d))
            .map(|&(i, _)| i);
        let count: usize = members
            .iter()
            .filter(|&&(i, d)| Some(i) == used && enabled(i, d))
            .map(|(_, d)| d.count)
            .sum();
        let site_id = site.to_owned();
        let targets: Vec<(String, String)> = members
            .iter()
            .map(|&(i, d)| (sets[i].id.clone(), d.domain.clone()))
            .collect();
        let (hover, press) = (theme.secondary_hover, theme.secondary_active);
        div()
            .id(SharedString::from(format!("ck-site-{site}")))
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(36.))
            .border_t_1()
            .border_color(theme.border)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .active(move |style| style.bg(press))
            .when(!any_on, |this| this.text_color(theme.muted_foreground))
            .on_click(cx.listener(move |this, _, _, cx| {
                let open_sites = &mut this.cookie_page.open_sites;
                if !open_sites.remove(&site_id) {
                    open_sites.insert(site_id.clone());
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        svg()
                            .path(icons::path(if open {
                                "icons/chevron-down.svg"
                            } else {
                                "icons/chevron-right.svg"
                            }))
                            .size(px(14.))
                            .flex_none()
                            .text_color(theme.muted_foreground),
                    )
                    .child(div().truncate().child(site.to_owned()))
                    .child(
                        div()
                            .text_size(theme.text(Text::Small))
                            .text_color(theme.muted_foreground)
                            .child(t!("set-ck-site-lines", count = members.len())),
                    ),
            )
            .child(
                div()
                    .w(px(140.))
                    .truncate()
                    .text_size(theme.text(Text::Small))
                    .text_color(theme.muted_foreground)
                    .child(used.map(|i| sets[i].name.clone()).unwrap_or_default()),
            )
            .child(
                div()
                    .w(px(56.))
                    .flex()
                    .justify_end()
                    .font_features(tabular())
                    .child(count.to_string()),
            )
            .child(div().w(px(92.)))
            .child(
                div().w(px(32.)).flex().justify_end().child(
                    Switch::new(SharedString::from(format!("ck-site-on-{site}")), any_on)
                        .name(t!("set-ck-site-on", site = site))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let targets = targets.clone();
                            this.edit_sets(
                                |sets| {
                                    for (id, domain) in &targets {
                                        if let Some(set) = sets.iter_mut().find(|s| s.id == *id) {
                                            set.off_domains.retain(|d| d != domain);
                                            if any_on {
                                                set.off_domains.push(domain.clone());
                                            }
                                        }
                                    }
                                },
                                cx,
                            );
                        })),
                ),
            )
            .into_any_element()
    }

    /// One domain of one set: the set it comes from (or which set wins), count, expiry, switch.
    fn domain_line(
        &self,
        sets: &[CookieSet],
        i: usize,
        d: &DomainSummary,
        nested: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let enabled =
            |set: &CookieSet, domain: &str| set.on && !set.off_domains.iter().any(|d| d == domain);
        let set = &sets[i];
        let on = enabled(set, &d.domain);
        let winner = sets.iter().position(|other| {
            enabled(other, &d.domain)
                && self
                    .cookie_page
                    .domains
                    .get(&other.id)
                    .and_then(|x| x.as_ref())
                    .is_some_and(|ds| ds.iter().any(|x| x.domain == d.domain))
        });
        let covered = on && winner.is_some_and(|w| w != i);
        let from = match winner {
            Some(w) if covered => t!("set-ck-covered", set = &sets[w].name),
            _ => SharedString::from(set.name.clone()),
        };
        let now_s = now_ms() / 1000;
        let expires: (SharedString, bool) = match d.expires {
            None => (t!("set-ck-session"), false),
            Some(at) if at < now_s => (t!("set-ck-expired"), true),
            // Chrome writes expiries centuries ahead: saturate, and a date past jiff's range shows blank
            Some(at) => (
                SharedString::from(format::when(at.saturating_mul(1000)).1.unwrap_or_default()),
                false,
            ),
        };
        let (id, domain) = (set.id.clone(), d.domain.clone());
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(36.))
            .border_t_1()
            .border_color(theme.border)
            .when(!on || covered, |this| {
                this.text_color(theme.muted_foreground)
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(nested, |this| {
                        this.pl(px(20.)).text_size(theme.text(Text::Small))
                    })
                    .child(d.domain.clone()),
            )
            .child(
                div()
                    .w(px(140.))
                    .truncate()
                    .text_size(theme.text(Text::Small))
                    .text_color(theme.muted_foreground)
                    .child(from),
            )
            .child(
                div()
                    .w(px(56.))
                    .flex()
                    .justify_end()
                    .font_features(tabular())
                    .child(d.count.to_string()),
            )
            .child(
                div()
                    .w(px(92.))
                    .text_size(theme.text(Text::Small))
                    .font_features(tabular())
                    .text_color(if expires.1 {
                        theme.danger
                    } else {
                        theme.muted_foreground
                    })
                    .child(expires.0),
            )
            .child(
                div().w(px(32.)).flex().justify_end().child(
                    Switch::new(SharedString::from(format!("ck-dom-{id}-{domain}")), on)
                        .name(t!("set-ck-domain-on", domain = &domain, set = &set.name))
                        .disabled(!set.on)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let (id, domain) = (id.clone(), domain.clone());
                            this.edit_sets(
                                |sets| {
                                    if let Some(set) = sets.iter_mut().find(|s| s.id == id) {
                                        match set.off_domains.iter().position(|d| *d == domain) {
                                            Some(at) => {
                                                set.off_domains.remove(at);
                                            }
                                            None => set.off_domains.push(domain),
                                        }
                                    }
                                },
                                cx,
                            );
                        })),
                ),
            )
            .into_any_element()
    }

    fn sets_section(&self, s: &Settings, cx: &mut Context<Self>) -> AnyElement {
        let sets = &s.cookies.sets;
        let last = sets.len().saturating_sub(1);
        let reading = self.cookie_page.reading.is_some();
        let mut rows: Vec<AnyElement> = vec![];
        for (at, set) in sets.iter().enumerate() {
            let id = set.id.clone();
            let source = match &set.source {
                CookieSource::File => t!("set-ck-from-file"),
                CookieSource::Paste => t!("set-ck-from-paste"),
                CookieSource::Browser(_) => t!("set-ck-from-browser"),
            };
            let detail = match self.cookie_page.domains.get(&set.id) {
                Some(None) => t!("set-ck-unread"),
                found => {
                    let domains = found.and_then(|d| d.as_ref()).map_or(0, |d| d.len());
                    let (time, date) = format::when(set.updated_ms);
                    t!(
                        "set-ck-set-meta",
                        source = &source,
                        domains = domains,
                        when = date.unwrap_or(time)
                    )
                }
            };
            let icon = |name: &str, path: &'static str, label: SharedString| {
                Button::new(SharedString::from(format!("ck-{name}-{id}")))
                    .ghost()
                    .small()
                    .icon(path)
                    .name(label)
            };
            let controls = div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    icon("up", "icons/chevron-up.svg", t!("set-ck-up"))
                        .disabled(at == 0)
                        .on_click(cx.listener(move |this, _, _, cx| this.move_set(at, true, cx))),
                )
                .child(
                    icon("down", "icons/chevron-down.svg", t!("set-ck-down"))
                        .disabled(at == last)
                        .on_click(cx.listener(move |this, _, _, cx| this.move_set(at, false, cx))),
                )
                .when_some(
                    match &set.source {
                        CookieSource::Browser(b) => Some(b.clone()),
                        _ => None,
                    },
                    |this, browser| {
                        let id = id.clone();
                        this.child(
                            Button::new(SharedString::from(format!("ck-refresh-{id}")))
                                .ghost()
                                .small()
                                .label(t!("set-ck-refresh"))
                                .disabled(reading)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.read_browser(browser.clone(), Some(id.clone()), cx)
                                })),
                        )
                    },
                )
                .child({
                    let id = id.clone();
                    Button::new(SharedString::from(format!("ck-edit-{id}")))
                        .ghost()
                        .small()
                        .label(t!("set-ck-edit"))
                        .on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.open_editor(&id, window, cx)
                            }),
                        )
                })
                .child({
                    let id = id.clone();
                    Button::new(SharedString::from(format!("ck-remove-{id}")))
                        .ghost()
                        .small()
                        .destructive()
                        .label(t!("set-ck-remove"))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_set(&id, cx)))
                })
                .child({
                    let id = id.clone();
                    Switch::new(SharedString::from(format!("ck-on-{id}")), set.on)
                        .name(t!("set-ck-on"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = id.clone();
                            this.edit_sets(
                                |sets| {
                                    if let Some(set) = sets.iter_mut().find(|s| s.id == id) {
                                        set.on = !set.on;
                                    }
                                },
                                cx,
                            )
                        }))
                });
            rows.push(row(
                SharedString::from(set.name.clone()),
                Some(detail),
                controls,
                cx,
            ));
        }
        if rows.is_empty() {
            rows.push(row(t!("set-cookies-empty"), None, div(), cx));
        }
        section(Some(t!("set-ck-sets")), rows, cx)
    }

    fn editor_section(&self, editor: &Editor, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let (status, bad) = match &editor.status {
            Ok(ok) => (ok.clone(), false),
            Err(why) => (why.clone(), true),
        };
        let changed = editor.area.read(cx).text() != editor.original;
        let original = editor.original.clone();
        let area = editor.area.clone();
        let footer = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .py(px(10.))
            .child(
                div()
                    .flex_1()
                    .text_size(theme.text(Text::Small))
                    .text_color(if bad {
                        theme.danger
                    } else {
                        theme.muted_foreground
                    })
                    .child(status),
            )
            .child(
                Button::new("ck-editor-close")
                    .ghost()
                    .label(t!("set-ck-close"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cookie_page.editor = None;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("ck-editor-revert")
                    .outline()
                    .label(t!("set-ck-revert"))
                    .disabled(!changed)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        area.update(cx, |area, cx| area.set_text(original.clone(), cx))
                    })),
            )
            .child(
                Button::new("ck-editor-save")
                    .primary()
                    .label(t!("set-ck-save"))
                    .disabled(bad || !changed)
                    .on_click(cx.listener(|this, _, _, cx| this.save_editor(cx))),
            );
        section(
            Some(t!("set-ck-edit-title", set = name)),
            vec![
                stacked(
                    t!("set-ck-edit-desc"),
                    None,
                    div()
                        .font_family(MONO)
                        .text_size(theme.text(Text::Small))
                        .child(editor.area.clone()),
                    cx,
                ),
                footer.into_any_element(),
            ],
            cx,
        )
    }
}
