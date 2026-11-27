//! Platform layer: single instance, tray, notifications, deep-link registration, clipboard
//! watcher and debug stats. State and views never touch this; they receive [`Event`]s through the
//! `on_event` hook given to [`Platform::install`].
// shortcut: the notifier and close-to-tray setters are API for the UI and state and have no caller yet,
// drop this allow when they do.
#![allow(dead_code)]

pub mod clipboard;
pub mod notify;
pub mod register;
pub mod stats;
mod tray;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use cli::args::{Command, Launch};
use cli::single::{self, Instance};
use cli::wire::Request;
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{App, Global, Task, Window};

/// Something the platform layer wants the app to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Links to add (command line, deep link, or a second launch). The window is shown too.
    AddUrls(Vec<String>),
    /// Links found on the clipboard when the window was activated (offer, never add silently).
    ClipboardLinks(Vec<String>),
    /// Bring the window to the front.
    Show,
    /// Quit the app (tray menu).
    Quit,
}

impl From<Request> for Event {
    fn from(r: Request) -> Event {
        match r {
            Request::Add { urls } => Event::AddUrls(urls),
            Request::Show => Event::Show,
        }
    }
}

/// Handler for every [`Event`].
pub type EventHandler = Box<dyn Fn(&Event, &mut App)>;

/// What the app gives the platform layer.
pub struct Hooks {
    /// Open the main window (called when it is asked for and none exists).
    pub open_window: Box<dyn Fn(&mut App)>,
    /// Every event, after the platform's own handling (show window, quit). State routes them here.
    pub on_event: EventHandler,
}

/// The first instance's platform state before the app runs.
pub struct Platform {
    tx: UnboundedSender<Event>,
    rx: UnboundedReceiver<Event>,
    minimized: bool,
    initial: Vec<String>,
    first: bool,
}

/// Claim the single-instance socket. `None` means another instance took over and this process
/// should exit; this returns in well under 2 s in that case.
pub fn start(
    launch: &Launch,
    command: Option<&Command>,
    data_dir: Option<&Path>,
) -> Option<Platform> {
    let urls = command.map(Command::urls).unwrap_or_default();
    let request = if urls.is_empty() {
        Request::Show
    } else {
        Request::Add { urls: urls.clone() }
    };
    let (tx, rx) = unbounded();
    let sender = tx.clone();
    let socket = single::socket_name(storage::app_dir_name(), data_dir);
    // A plain launch with no link must not steal focus from a running app only to find nothing to do,
    // but showing it is exactly what a user expects from launching the app again.
    match single::claim(&socket, &request, move |r| {
        drop(sender.unbounded_send(r.into()))
    }) {
        Instance::Running => None,
        first @ (Instance::First | Instance::Failed) => Some(Platform {
            tx,
            rx,
            minimized: launch.minimized,
            initial: urls,
            first: matches!(first, Instance::First),
        }),
    }
}

struct Installed {
    tray: Option<tray::Tray>,
    close_to_tray: Cell<bool>,
    notifier: RefCell<Rc<dyn notify::Notifier>>,
    tx: UnboundedSender<Event>,
    keep: RefCell<Vec<Box<dyn Any>>>,
    _pump: Task<()>,
}

impl Global for Installed {}

impl Platform {
    /// Whether this process owns the single-instance socket (not a failed claim); only that one
    /// may open the browser-extension port.
    pub fn is_first(&self) -> bool {
        self.first
    }

    /// Sets up the tray, notifications, event pump and (release builds) protocol registration.
    /// Returns whether the app should open its first window now (false for `--minimized` with a tray).
    pub fn install(self, hooks: Hooks, register_protocol: bool, cx: &mut App) -> bool {
        let Platform {
            tx,
            mut rx,
            minimized,
            initial,
            ..
        } = self;
        let tray = tray::Tray::new(tx.clone());
        let open = !(minimized && tray.is_some());
        if register_protocol {
            std::thread::spawn(|| {
                if let Err(e) = register::ensure() {
                    log::warn!("platform: cannot register the ytdlpd:// scheme: {e:#}");
                }
            });
        }
        if !initial.is_empty() {
            tx.unbounded_send(Event::AddUrls(initial)).ok();
        }
        let hooks = Rc::new(hooks);
        let pump = cx.spawn(async move |cx| {
            while let Some(event) = rx.next().await {
                let hooks = hooks.clone();
                cx.update(|cx| {
                    match &event {
                        Event::Quit => cx.quit(),
                        Event::Show | Event::AddUrls(_) => show(&hooks, cx),
                        Event::ClipboardLinks(_) => {}
                    }
                    (hooks.on_event)(&event, cx);
                });
            }
        });
        cx.set_global(Installed {
            tray,
            close_to_tray: Cell::new(false),
            notifier: RefCell::new(Rc::new(notify::Desktop)),
            tx,
            keep: RefCell::new(Vec::new()),
            _pump: pump,
        });
        open
    }
}

/// Sends `event` through the event pump, as if it came from a second launch.
pub fn emit(event: Event, cx: &App) {
    if let Some(p) = cx.try_global::<Installed>() {
        p.tx.unbounded_send(event).ok();
    }
}

/// Raise the existing window or open one.
fn show(hooks: &Hooks, cx: &mut App) {
    match cx.windows().first().copied() {
        Some(w) => {
            w.update(cx, |_, window, _| window.activate_window()).ok();
        }
        None => (hooks.open_window)(cx),
    }
    cx.activate(true);
}

/// Whether closing the last window keeps the app running (a tray exists and the setting is on).
pub fn keeps_running(cx: &App) -> bool {
    cx.try_global::<Installed>()
        .is_some_and(|p| p.tray.is_some() && p.close_to_tray.get())
}

/// The "close to tray" setting (wired to settings); only effective with a tray.
pub fn set_close_to_tray(on: bool, cx: &App) {
    if let Some(p) = cx.try_global::<Installed>() {
        p.close_to_tray.set(on);
    }
}

/// The notifier (desktop by default).
pub fn notifier(cx: &App) -> Rc<dyn notify::Notifier> {
    cx.try_global::<Installed>().map_or_else(
        || Rc::new(notify::Desktop) as _,
        |p| p.notifier.borrow().clone(),
    )
}

/// Replace the notifier (tests, demo scenes).
pub fn set_notifier(n: Rc<dyn notify::Notifier>, cx: &App) {
    if let Some(p) = cx.try_global::<Installed>() {
        *p.notifier.borrow_mut() = n;
    }
}

/// Per-window hooks, called from the window-creation closure: clipboard watch on activation.
pub fn attach_window(window: &mut Window, cx: &mut App) {
    let Some(p) = cx.try_global::<Installed>() else {
        return;
    };
    let tx = p.tx.clone();
    let watch = clipboard::attach(window, cx, move |links, _| {
        drop(tx.unbounded_send(Event::ClipboardLinks(links)))
    });
    if let Some(p) = cx.try_global::<Installed>() {
        p.keep.borrow_mut().push(Box::new(watch));
    }
}
