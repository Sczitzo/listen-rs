//! Notification-area (tray) icon and menu, as in the original (dictate.py ~3120–3140, 2400).
//!
//! Original menu: "Open Listen" (default, also on click), favourites submenu ("(none saved yet)"
//! disabled when empty, each item "{name}  ({uses})" copies the text), and Quit. The icon shows
//! the Listen mark coloured by state; the tooltip reads "Listen: <status>".
//! Quit closes the app through the same save check as the window's close button.

use crate::mark;
use crate::storage::Favorite;
use std::sync::mpsc::{channel, Receiver};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Debug, Clone, PartialEq)]
pub enum TrayAction {
    Open,
    PromptLab,
    Settings,
    CopyFavorite(usize),
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrayState {
    Loading,
    Idle,
    Recording,
    Working,
}

impl TrayState {
    fn color(self) -> [u8; 3] {
        match self {
            TrayState::Loading => [0x8E, 0x88, 0x80],
            TrayState::Idle => mark::IDLE,
            TrayState::Recording => [0xE5, 0x48, 0x4D],
            TrayState::Working => [0xF5, 0xA5, 0x24],
        }
    }
}

pub struct Tray {
    icon: TrayIcon,
    state: Option<TrayState>,
    tooltip: String,
    /// Names and use counts the menu was last built from, to rebuild only on change.
    fav_key: Vec<(String, u32)>,
    rx: Receiver<TrayAction>,
}

fn make_icon(state: TrayState) -> Option<Icon> {
    let size = 32;
    Icon::from_rgba(mark::rgba(size, state.color()), size, size).ok()
}

fn fav_label(f: &Favorite) -> String {
    // Same label as the original: two spaces, then the use count in brackets.
    let name: String = f.name.chars().take(60).collect();
    format!("{name}  ({})", f.uses)
}

fn build_menu(favs: &[Favorite]) -> Menu {
    let menu = Menu::new();
    let _ = menu.append(&MenuItem::with_id("open", "Open Listen", true, None));
    let _ = menu.append(&MenuItem::with_id("lab", "Prompt Lab", true, None));
    let sub = Submenu::new("Copy favourite", true);
    if favs.is_empty() {
        let _ = sub.append(&MenuItem::new("(none saved yet)", false, None));
    } else {
        for (i, f) in favs.iter().enumerate() {
            let _ = sub.append(&MenuItem::with_id(format!("fav:{i}"), fav_label(f), true, None));
        }
    }
    let _ = menu.append(&sub);
    let _ = menu.append(&MenuItem::with_id("settings", "Settings", true, None));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id("quit", "Quit", true, None));
    menu
}

impl Tray {
    /// Queue the action before waking the app. The tray callback must return without changing windows.
    pub fn new(favs: &[Favorite], wake: impl Fn() + Send + Sync + 'static) -> Result<Tray, String> {
        let (tx, rx) = channel::<TrayAction>();
        let wake = std::sync::Arc::new(wake);
        {
            let tx = tx.clone();
            let wake = wake.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let id = e.id().0.as_str();
                let action = match id {
                    "open" => Some(TrayAction::Open),
                    "lab" => Some(TrayAction::PromptLab),
                    "settings" => Some(TrayAction::Settings),
                    "quit" => Some(TrayAction::Quit),
                    s => s.strip_prefix("fav:").and_then(|n| n.parse().ok()).map(TrayAction::CopyFavorite),
                };
                if let Some(a) = action {
                    if tx.send(a).is_ok() {
                        wake();
                    }
                }
            }));
        }
        {
            let wake = wake.clone();
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                // "Open Listen" is the default item: a left click (or double click) opens the window.
                let open = match e {
                    TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => true,
                    TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => true,
                    _ => false,
                };
                if open && tx.send(TrayAction::Open).is_ok() {
                    wake();
                }
            }));
        }
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(build_menu(favs)))
            .with_menu_on_left_click(false)
            .with_tooltip("Listen: loading")
            .with_icon(make_icon(TrayState::Loading).ok_or("icon")?)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Tray {
            icon,
            state: Some(TrayState::Loading),
            tooltip: "Listen: loading".into(),
            fav_key: favs.iter().map(|f| (f.name.clone(), f.uses)).collect(),
            rx,
        })
    }

    pub fn poll(&self) -> Vec<TrayAction> {
        self.rx.try_iter().collect()
    }

    /// Recolour the icon, retitle the tooltip and rebuild the favourites menu only when they change.
    pub fn sync(&mut self, state: TrayState, status: &str, favs: &[Favorite]) {
        if self.state != Some(state) {
            let _ = self.icon.set_icon(make_icon(state));
            self.state = Some(state);
        }
        // Windows limits tooltips to 127 characters.
        let tip: String = format!("Listen: {status}").chars().take(120).collect();
        if tip != self.tooltip {
            let _ = self.icon.set_tooltip(Some(&tip));
            self.tooltip = tip;
        }
        let key: Vec<(String, u32)> = favs.iter().map(|f| (f.name.clone(), f.uses)).collect();
        if key != self.fav_key {
            self.icon.set_menu(Some(Box::new(build_menu(favs))));
            self.fav_key = key;
        }
    }
}
