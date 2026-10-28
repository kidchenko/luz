//! f.lux's keyboard shortcuts, system-wide, so someone switching over keeps
//! their muscle memory:
//!
//! | keys              | does                      |
//! |-------------------|---------------------------|
//! | Alt+End           | pause for an hour, resume |
//! | Alt+Shift+PgDn/Up | warmer / cooler           |
//! | Alt+PgDn/Up       | dimmer / brighter         |
//!
//! Alt is Option on a Mac, and End and PgUp/PgDn are fn with the arrow keys
//! on a laptop keyboard. A shortcut another app already holds (f.lux itself,
//! most likely) is skipped and logged; the rest still work.

use crate::app::UserEvent;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tao::event_loop::EventLoopProxy;
use tray_icon::menu::accelerator::Accelerator;
use traykit::{linfo, lwarn};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Pause,
    Warmer,
    Cooler,
    Dimmer,
    Brighter,
}

const ALL: [Action; 5] = [
    Action::Pause,
    Action::Warmer,
    Action::Cooler,
    Action::Dimmer,
    Action::Brighter,
];

impl Action {
    fn hotkey(self) -> HotKey {
        let shift = Modifiers::ALT | Modifiers::SHIFT;
        let (mods, code) = match self {
            Action::Pause => (Modifiers::ALT, Code::End),
            Action::Warmer => (shift, Code::PageDown),
            Action::Cooler => (shift, Code::PageUp),
            Action::Dimmer => (Modifiers::ALT, Code::PageDown),
            Action::Brighter => (Modifiers::ALT, Code::PageUp),
        };
        HotKey::new(Some(mods), code)
    }

    /// The same keys as a menu shortcut, which macOS draws in the menu's
    /// shortcut column.
    pub fn accelerator(self) -> Accelerator {
        let key = self.hotkey();
        Accelerator::new(key.mods, key.key)
    }

    /// How Windows menus and the log write it, the way each OS writes
    /// shortcuts.
    pub fn label(self) -> &'static str {
        if cfg!(target_os = "macos") {
            match self {
                Action::Pause => "\u{2325}End",
                Action::Warmer => "\u{2325}\u{21e7}\u{21df}",
                Action::Cooler => "\u{2325}\u{21e7}\u{21de}",
                Action::Dimmer => "\u{2325}\u{21df}",
                Action::Brighter => "\u{2325}\u{21de}",
            }
        } else {
            match self {
                Action::Pause => "Alt+End",
                Action::Warmer => "Alt+Shift+PgDn",
                Action::Cooler => "Alt+Shift+PgUp",
                Action::Dimmer => "Alt+PgDn",
                Action::Brighter => "Alt+PgUp",
            }
        }
    }
}

pub struct Hotkeys {
    manager: Option<GlobalHotKeyManager>,
    on: Vec<HotKey>,
}

impl Hotkeys {
    /// Must be made on the event loop's thread: on Windows the hotkeys are
    /// delivered to a window that thread owns, and on macOS through its run
    /// loop.
    pub fn new(proxy: &EventLoopProxy<UserEvent>) -> Self {
        let manager = match GlobalHotKeyManager::new() {
            Ok(m) => Some(m),
            Err(e) => {
                lwarn!("no hotkeys: {e}");
                None
            }
        };
        let proxy = proxy.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            // Held down, Windows repeats the press, which makes holding
            // Alt+PgDn keep dimming. macOS doesn't, so on a Mac it's a press
            // per step.
            if e.state == HotKeyState::Pressed {
                if let Some(a) = ALL.iter().find(|a| a.hotkey().id() == e.id) {
                    let _ = proxy.send_event(UserEvent::Hotkey(*a));
                }
            }
        }));
        Self {
            manager,
            on: Vec::new(),
        }
    }

    /// Register or drop them all.
    pub fn enable(&mut self, want: bool) {
        let Some(m) = &self.manager else { return };
        if !want {
            if !self.on.is_empty() {
                let _ = m.unregister_all(&self.on);
                self.on.clear();
                linfo!("hotkeys off");
            }
            return;
        }
        if !self.on.is_empty() {
            return;
        }
        for a in ALL {
            let key = a.hotkey();
            match m.register(key) {
                Ok(()) => self.on.push(key),
                Err(e) => lwarn!("{} is taken ({e}); skipping it", a.label()),
            }
        }
        linfo!("hotkeys on ({} of {})", self.on.len(), ALL.len());
    }
}
