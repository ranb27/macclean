//! Menu-bar tray — `--tray` keeps macclean resident in the macOS menu
//! bar. The tray menu offers Scan / Show / Quit; closing the window then
//! hides it instead of quitting, so Quit only lives here.

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// What a tray interaction asks the app to do.
pub enum Command {
    Scan,
    Show,
    Quit,
}

pub struct Tray {
    /// Keep alive — dropping removes the status item.
    _icon: TrayIcon,
    scan: MenuId,
    show: MenuId,
    quit: MenuId,
}

/// Install the status item. Must run on the main thread (tray-icon
/// refuses otherwise); we call it lazily from the first `ui()` pass.
/// Failure just disables tray mode — the window still works.
pub fn install() -> Result<Tray, String> {
    let menu = Menu::new();
    let show = MenuItem::new("Show macclean", true, None);
    let scan = MenuItem::new("Scan now", true, None);
    let quit = MenuItem::new("Quit macclean", true, None);
    menu.append(&show).map_err(|e| e.to_string())?;
    menu.append(&scan).map_err(|e| e.to_string())?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| e.to_string())?;
    menu.append(&quit).map_err(|e| e.to_string())?;

    // Reuse the app icon — eframe's decoder hands us raw RGBA, no image
    // dependency needed.
    let data = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-1024.png"))
        .map_err(|e| e.to_string())?;
    let icon = tray_icon::Icon::from_rgba(data.rgba, data.width, data.height)
        .map_err(|e| e.to_string())?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("macclean")
        .with_icon(icon)
        .with_menu_on_left_click(true)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(Tray {
        _icon: tray,
        scan: scan.id().clone(),
        show: show.id().clone(),
        quit: quit.id().clone(),
    })
}

impl Tray {
    /// Drain pending tray/menu events into commands. Menu clicks don't
    /// wake winit — callers repaint on a timer while a tray exists.
    pub fn poll(&self) -> Vec<Command> {
        let mut cmds = Vec::new();
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == self.scan {
                cmds.push(Command::Scan);
            } else if ev.id == self.show {
                cmds.push(Command::Show);
            } else if ev.id == self.quit {
                cmds.push(Command::Quit);
            }
        }
        // Left-click on the icon itself also raises the window — the menu
        // still shows because with_menu_on_left_click only fires events.
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Down,
                ..
            } = ev
            {
                cmds.push(Command::Show);
            }
        }
        cmds
    }
}
