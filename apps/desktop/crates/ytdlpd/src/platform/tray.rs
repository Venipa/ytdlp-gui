//! Tray icon with Show and Quit. tray-icon on Windows and macOS, ksni (StatusNotifierItem) on Linux.
use futures::channel::mpsc::UnboundedSender;

use super::Event;

/// 64x64 RGBA tray icon, written by `scripts/sync-icons.py`. macOS gets the monotone template
/// (black plus alpha), which the menu bar tints for light and dark.
#[cfg(not(target_os = "macos"))]
const ICON: &[u8] = include_bytes!("../../../../assets/tray/icon-64.rgba");
#[cfg(target_os = "macos")]
const ICON: &[u8] = include_bytes!("../../../../assets/tray/icon-template-64.rgba");
const ICON_SIZE: u32 = 64;

#[cfg(any(target_os = "macos", windows))]
pub struct Tray {
    _icon: tray_icon::TrayIcon,
}

#[cfg(any(target_os = "macos", windows))]
impl Tray {
    /// Builds the tray icon. `None` when the platform refuses (logged).
    pub fn new(tx: UnboundedSender<Event>) -> Option<Tray> {
        use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
        use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

        let build = || -> anyhow::Result<tray_icon::TrayIcon> {
            use anyhow::Context as _;
            let show = MenuItem::with_id("show", i18n::t!("tray-show"), true, None);
            let quit = MenuItem::with_id("quit", i18n::t!("tray-quit"), true, None);
            let menu = Menu::new();
            menu.append_items(&[&show, &PredefinedMenuItem::separator(), &quit])
                .context("cannot build the tray menu")?;
            let (menu_tx, click_tx) = (tx.clone(), tx);
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| match e.id.0.as_str() {
                "show" => drop(menu_tx.unbounded_send(Event::Show)),
                "quit" => drop(menu_tx.unbounded_send(Event::Quit)),
                _ => {}
            }));
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = e
                {
                    click_tx.unbounded_send(Event::Show).ok();
                }
            }));
            let icon = tray_icon::Icon::from_rgba(ICON.to_vec(), ICON_SIZE, ICON_SIZE)
                .context("cannot build the tray icon")?;
            // the macOS menu bar tints a template image for light and dark; the method exists only
            // in macOS builds of tray-icon, so it is chosen at compile time
            #[cfg(target_os = "macos")]
            let builder = TrayIconBuilder::new().with_icon_templated(icon);
            #[cfg(not(target_os = "macos"))]
            let builder = TrayIconBuilder::new().with_icon(icon);
            builder
                .with_tooltip(i18n::t!("tray-tooltip"))
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .build()
                .context("cannot build the status item")
        };
        match build() {
            Ok(_icon) => Some(Tray { _icon }),
            Err(e) => {
                log::warn!("platform: no tray icon: {e:#}");
                None
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub struct Tray {
    _handle: ksni::blocking::Handle<Item>,
}

#[cfg(target_os = "linux")]
struct Item {
    tx: UnboundedSender<Event>,
    pixmap: Vec<ksni::Icon>,
    show: String,
    quit: String,
    tooltip: String,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for Item {
    fn id(&self) -> String {
        "ytdlpd".into()
    }

    fn title(&self) -> String {
        self.tooltip.clone()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.pixmap.clone()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.tx.unbounded_send(Event::Show).ok();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: self.show.clone(),
                activate: Box::new(|t: &mut Self| drop(t.tx.unbounded_send(Event::Show))),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: self.quit.clone(),
                activate: Box::new(|t: &mut Self| drop(t.tx.unbounded_send(Event::Quit))),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(target_os = "linux")]
impl Tray {
    /// Registers a StatusNotifierItem. `None` without a host on the session bus (logged).
    pub fn new(tx: UnboundedSender<Event>) -> Option<Tray> {
        use ksni::blocking::TrayMethods as _;

        // StatusNotifierItem wants ARGB32 in network order; the asset is RGBA
        let mut data = ICON.to_vec();
        for px in data.as_chunks_mut::<4>().0 {
            px.rotate_right(1);
        }
        let item = Item {
            tx,
            pixmap: vec![ksni::Icon {
                width: ICON_SIZE as i32,
                height: ICON_SIZE as i32,
                data,
            }],
            show: i18n::t!("tray-show").to_string(),
            quit: i18n::t!("tray-quit").to_string(),
            tooltip: i18n::t!("tray-tooltip").to_string(),
        };
        match item.spawn() {
            Ok(_handle) => Some(Tray { _handle }),
            Err(e) => {
                log::warn!("platform: no tray icon: {e}");
                None
            }
        }
    }
}
