//! The tray icon, its menu, and the minute-by-minute colour.
//!
//! Everything here runs on the event loop's thread. Once a second the app
//! works out where the colour should be (the schedule, then any pause or
//! adjustment on top) and hands it to the screen thread, which does the
//! fading. The menu's top two lines change in place; the rest is rebuilt only
//! when something on it changed, and on macOS not while it is open, because
//! replacing an open menu closes it.

use crate::backlight::{Backlight, Monitor};
use crate::color::{self, MAX_K, MIN_K, NEUTRAL_K};
use crate::config::{Config, FAST_FADE, SLOW_FADE};
use crate::hotkeys::{Action, Hotkeys};
use crate::icon::{self, Face};
use crate::log::{lerror, linfo, lwarn};
use crate::place::Place;
use crate::schedule::{self, Colours, Event, Phase, Plan};
use crate::screen::{Screen, Shown};
use crate::sun::{self, Day};
use crate::util::{clock_label, local_now, now_unix, span, Local};
use crate::{art, autostart, gamma, notify, rival, theme};

use std::time::{Duration, Instant, SystemTime};
use tao::event_loop::EventLoopProxy;
use tray_icon::menu::accelerator::Accelerator;
use tray_icon::menu::{
    CheckMenuItem, IconMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const ID_PAUSE_HOUR: &str = "pause-hour";
const ID_PAUSE_MORNING: &str = "pause-morning";
const ID_RESUME: &str = "resume";
const ID_WARMER: &str = "warmer";
const ID_COOLER: &str = "cooler";
const ID_DIMMER: &str = "dimmer";
const ID_BRIGHTER: &str = "brighter";
const ID_BACKLIGHT: &str = "backlight:";
const ID_UNDO: &str = "undo";
const ID_DAY_K: &str = "day-k:";
const ID_SUNSET_K: &str = "sunset-k:";
const ID_BEDTIME_K: &str = "bedtime-k:";
const ID_BEDTIME: &str = "bedtime";
const ID_BED_AT: &str = "bed-at:";
const ID_WAKE_AT: &str = "wake-at:";
const ID_FADE: &str = "fade:";
const ID_HOTKEYS: &str = "hotkeys";
const ID_AUTOSTART: &str = "autostart";
const ID_UNLOCK: &str = "unlock";
const ID_EDIT: &str = "edit-config";
const ID_UPDATES: &str = "check-updates";
const ID_QUIT: &str = "quit";
/// Shared by every read-only row; nothing dispatches on it.
const ID_INERT: &str = "inert";

const DAY_PRESETS: [u32; 4] = [6500, 5500, 5000, 4200];
const SUNSET_PRESETS: [u32; 5] = [4200, 3400, 2700, 2300, 1900];
const BEDTIME_PRESETS: [u32; 4] = [2700, 2300, 1900, 1200];
const BED_TIMES: [&str; 9] = [
    "21:00", "21:30", "22:00", "22:30", "23:00", "23:30", "00:00", "00:30", "01:00",
];
const WAKE_TIMES: [&str; 10] = [
    "05:00", "05:30", "06:00", "06:30", "07:00", "07:30", "08:00", "08:30", "09:00", "09:30",
];

/// One press of Warmer or Cooler, in mireds: about 250 K at sunset colours,
/// 600 K in daylight and 70 K by candlelight, which the eye sees as the same
/// step.
const NUDGE: f64 = 20.0;
/// One press of Dimmer or Brighter, and how dim it goes.
const DIM_STEP: f64 = 0.1;
const DIM_FLOOR: f64 = 0.3;
/// One press of Dimmer or Brighter on a monitor's own backlight, in percent,
/// and the levels its menu offers.
const BACKLIGHT_STEP: i32 = 10;
const BACKLIGHT_PRESETS: [u8; 5] = [100, 75, 50, 25, 0];

/// How long the screen takes to follow a hand-made change, and the
/// schedule's own second-by-second one.
const QUICK: Duration = Duration::from_millis(400);
const GENTLE: Duration = Duration::from_millis(2000);
const STEADY: Duration = Duration::from_millis(1100);

/// How often to look for f.lux, until it has been mentioned once.
const RIVAL_EVERY: Duration = Duration::from_secs(30);

pub enum UserEvent {
    Menu(String),
    Hotkey(Action),
    /// Once a second, from a thread: time to see where the colour should be.
    Tick,
    /// A second launch of the app: pause, or resume.
    Summon,
    /// The backlight thread read new levels.
    Backlight,
}

#[derive(PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
}

/// Warmer/cooler and dimmer/brighter, on top of the schedule until it next
/// changes phase: a nudge at 21:00 lasts until bedtime, as f.lux's do.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Adjust {
    mireds: f64,
    brightness: f64,
    /// The phase they were made in.
    during: Phase,
}

/// The sun's day, and the date and place it was worked out for.
struct SunDay {
    day: Day,
    date: (i32, u32, u32),
    at: Option<(f64, f64)>,
}

pub struct App {
    cfg: Config,
    cfg_mtime: Option<SystemTime>,
    tray: TrayIcon,
    dark_taskbar: bool,
    /// Started once the first colour is known.
    screen: Option<Screen>,
    hotkeys: Hotkeys,
    backlight: Backlight,

    place: Option<Place>,
    sun: SunDay,
    now: Local,
    plan: Plan,
    adjust: Option<Adjust>,
    /// What was last sent to the screen thread.
    target: (f64, f64),

    rival_checked: Instant,
    rival_mentioned: bool,

    // What is on screen, so a tick only repaints what changed.
    face: Face,
    header: (String, String),
    header_items: Option<(IconMenuItem, MenuItem)>,
    menu_sig: String,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Result<Self, String> {
        forward_menu_events(&proxy);

        let cfg = Config::load();
        let place = Place::find(cfg.latitude, cfg.longitude);
        match &place {
            Some(p) => linfo!("location: {}", p.describe()),
            None => lwarn!("location unknown; using 07:00 to 19:00 for the sun"),
        }

        let dark_taskbar = theme::taskbar_is_dark();
        let icon = tray_image(Face::Day, dark_taskbar)?;
        // On macOS the icon is a template: AppKit tints it for the menu bar.
        #[cfg(target_os = "macos")]
        let builder = TrayIconBuilder::new().with_icon_templated(icon);
        #[cfg(windows)]
        let builder = TrayIconBuilder::new().with_icon(icon);
        let tray = builder
            .with_tooltip("Luz")
            // Either button opens the menu: it is the whole interface.
            .with_menu_on_left_click(true)
            .build()
            .map_err(|e| format!("could not create the tray icon: {e}"))?;

        let mut hotkeys = Hotkeys::new(&proxy);
        hotkeys.enable(cfg.hotkeys);

        let now = local_now();
        let mut app = Self {
            cfg_mtime: Config::modified(),
            tray,
            dark_taskbar,
            screen: None,
            hotkeys,
            backlight: {
                let proxy = proxy.clone();
                Backlight::start(move || {
                    let _ = proxy.send_event(UserEvent::Backlight);
                })
            },
            place,
            sun: SunDay {
                day: Day::AlwaysUp,
                date: (0, 0, 0),
                at: None,
            },
            now,
            plan: Plan {
                kelvin: NEUTRAL_K as f64,
                phase: Phase::Day,
                fading: false,
                next: None,
            },
            adjust: None,
            target: (NEUTRAL_K as f64, 1.0),
            rival_checked: Instant::now(),
            rival_mentioned: false,
            face: Face::Day,
            header: Default::default(),
            header_items: None,
            menu_sig: String::new(),
            cfg,
        };
        app.replan();
        app.target = app.wanted();
        // Straight to the colour of the hour rather than fading in from plain.
        app.screen = Some(Screen::start(app.target.0, app.target.1));
        linfo!(
            "{:?}, {:.0} K; next: {:?}",
            app.plan.phase,
            app.target.0,
            app.plan.next
        );
        app.check_rival();
        app.paint_icon(true);
        app.rebuild_menu(true);
        Ok(app)
    }

    pub fn handle(&mut self, event: UserEvent) -> Flow {
        match event {
            UserEvent::Tick => self.tick(),
            UserEvent::Backlight => self.rebuild_menu(false),
            UserEvent::Summon => {
                if self.paused() {
                    self.resume();
                    notify::show("Luz is back on", &self.status_line(), None);
                } else {
                    self.pause_for(3600);
                    notify::show(
                        "Luz is paused for an hour",
                        "Your screen is back to its own colours. Open Luz again to resume.",
                        None,
                    );
                }
            }
            UserEvent::Hotkey(a) => {
                linfo!("hotkey: {a:?}");
                match a {
                    Action::Pause if self.paused() => self.resume(),
                    Action::Pause => self.pause_for(3600),
                    Action::Warmer => self.nudge(NUDGE, 0.0),
                    Action::Cooler => self.nudge(-NUDGE, 0.0),
                    Action::Dimmer => self.dim(-1),
                    Action::Brighter => self.dim(1),
                }
            }
            UserEvent::Menu(id) => return self.menu(&id),
        }
        Flow::Continue
    }

    fn menu(&mut self, id: &str) -> Flow {
        match id {
            ID_QUIT => {
                linfo!("quit requested from the tray");
                return Flow::Exit;
            }
            ID_PAUSE_HOUR => self.pause_for(3600),
            ID_PAUSE_MORNING => {
                if let Some((_, wait, _)) = self.next_morning() {
                    self.pause_for((wait * 60.0).ceil() as u64);
                }
            }
            ID_RESUME => self.resume(),
            ID_WARMER => self.nudge(NUDGE, 0.0),
            ID_COOLER => self.nudge(-NUDGE, 0.0),
            ID_DIMMER => self.dim(-1),
            ID_BRIGHTER => self.dim(1),
            ID_UNDO => {
                self.adjust = None;
                self.follow(QUICK);
            }
            ID_BEDTIME => {
                self.cfg.bedtime = !self.cfg.bedtime;
                self.save_and_follow();
            }
            ID_HOTKEYS => {
                self.cfg.hotkeys = !self.cfg.hotkeys;
                self.hotkeys.enable(self.cfg.hotkeys);
                self.save();
            }
            ID_AUTOSTART => {
                let want = !autostart::is_enabled();
                match autostart::set(want) {
                    Ok(()) => linfo!("run at login {}", if want { "on" } else { "off" }),
                    Err(e) => {
                        lerror!("{e}");
                        notify::show("Couldn't change Run at login", &e, None);
                    }
                }
            }
            ID_UNLOCK => unlock_warmer(),
            ID_EDIT => open_config(),
            ID_UPDATES => check_for_updates(),
            other => {
                if let Some((i, level)) = other
                    .strip_prefix(ID_BACKLIGHT)
                    .and_then(|r| r.split_once(':'))
                    .and_then(|(i, l)| Some((i.parse().ok()?, l.parse().ok()?)))
                {
                    self.backlight.set(i, level);
                    return Flow::Continue;
                }
                if let Some(k) = parse::<u32>(other, ID_DAY_K) {
                    self.cfg.day_kelvin = k;
                } else if let Some(k) = parse::<u32>(other, ID_SUNSET_K) {
                    self.cfg.sunset_kelvin = k;
                } else if let Some(k) = parse::<u32>(other, ID_BEDTIME_K) {
                    self.cfg.bedtime_kelvin = k;
                } else if let Some(t) = other.strip_prefix(ID_BED_AT) {
                    self.cfg.bedtime_at = t.to_string();
                    self.cfg.bedtime = true;
                } else if let Some(t) = other.strip_prefix(ID_WAKE_AT) {
                    self.cfg.wake_at = t.to_string();
                } else if let Some(s) = parse::<u32>(other, ID_FADE) {
                    self.cfg.fade_seconds = s;
                } else {
                    return Flow::Continue;
                }
                self.cfg = std::mem::take(&mut self.cfg).sanitised();
                self.save_and_follow();
            }
        }
        self.rebuild_menu(true);
        Flow::Continue
    }

    fn tick(&mut self) {
        self.reload_if_edited();
        let dark = theme::taskbar_is_dark();
        if dark != self.dark_taskbar {
            self.dark_taskbar = dark;
            self.paint_icon(true);
        }
        if self.cfg.paused_until != 0 && !self.paused() {
            linfo!("pause over");
            self.cfg.paused_until = 0;
            self.save();
        }
        self.follow(STEADY);
        if !self.rival_mentioned && self.rival_checked.elapsed() >= RIVAL_EVERY {
            self.check_rival();
        }
        self.paint_icon(false);
        self.rebuild_menu(false);
    }

    /// Work out the plan for this minute.
    fn replan(&mut self) {
        self.now = local_now();
        let date = (self.now.year, self.now.month, self.now.day);
        let at = self.place.as_ref().map(|p| (p.lat, p.lon));
        if self.sun.date != date || self.sun.at != at {
            let day = match at {
                Some((lat, lon)) => sun::day(
                    lat,
                    lon,
                    sun::day_of_year(date.0, date.1, date.2),
                    self.now.utc_offset,
                ),
                // Nowhere to go by: a plain 7-to-7 day.
                None => Day::Normal {
                    rise: 7.0 * 60.0,
                    set: 19.0 * 60.0,
                },
            };
            if let Day::Normal { rise, set } = day {
                linfo!(
                    "sun for {}-{:02}-{:02}: up {}, down {}",
                    date.0,
                    date.1,
                    date.2,
                    clock_label(rise),
                    clock_label(set)
                );
            }
            self.sun = SunDay { day, date, at };
        }
        let before = self.plan.phase;
        self.plan = schedule::plan(self.now.minute, self.sun.day, &self.colours());
        if self.plan.phase != before {
            linfo!("now {:?}", self.plan.phase);
        }
        // Adjustments last until the schedule moves on.
        if self.adjust.is_some_and(|a| a.during != self.plan.phase) {
            linfo!("adjustments cleared for {:?}", self.plan.phase);
            self.adjust = None;
        }
    }

    fn colours(&self) -> Colours {
        Colours {
            day: self.cfg.day_kelvin as f64,
            sunset: self.cfg.sunset_kelvin as f64,
            bedtime: self.cfg.bedtime_kelvin as f64,
            bed: self.cfg.bedtime.then(|| {
                (
                    self.cfg.bedtime_minute() as f64,
                    self.cfg.wake_minute() as f64,
                )
            }),
            fade: self.cfg.fade_seconds as f64 / 60.0,
        }
    }

    fn paused(&self) -> bool {
        self.cfg.paused_until > now_unix()
    }

    /// Where the screen should be: the plan, then adjustments; or plain while
    /// paused.
    fn wanted(&self) -> (f64, f64) {
        if self.paused() {
            return (NEUTRAL_K as f64, 1.0);
        }
        match self.adjust {
            Some(a) => (
                (1e6 / (1e6 / self.plan.kelvin + a.mireds)).clamp(MIN_K as f64, MAX_K as f64),
                a.brightness,
            ),
            None => (self.plan.kelvin, 1.0),
        }
    }

    /// Re-plan and send the screen thread where to go, if anywhere new.
    fn follow(&mut self, over: Duration) {
        self.replan();
        let want = self.wanted();
        if want != self.target {
            // A change of mind (pause, a new colour) eases over its own time;
            // the schedule creeping along follows second by second.
            let jump = (1e6 / want.0 - 1e6 / self.target.0).abs() > 8.0
                || (want.1 - self.target.1).abs() > 0.05;
            let over = if jump && over == STEADY { GENTLE } else { over };
            if let Some(s) = &self.screen {
                s.go(want.0, want.1, over);
            }
            self.target = want;
        }
        self.update_header();
    }

    fn pause_for(&mut self, secs: u64) {
        self.cfg.paused_until = now_unix() + secs;
        linfo!("paused for {}", span(secs));
        self.save();
        self.follow(GENTLE);
        self.paint_icon(false);
        self.rebuild_menu(true);
    }

    fn resume(&mut self) {
        self.cfg.paused_until = 0;
        linfo!("resumed");
        self.save();
        self.follow(GENTLE);
        self.paint_icon(false);
        self.rebuild_menu(true);
    }

    /// Warmer/cooler by `mireds`, brighter/dimmer by `brightness`. Resumes a
    /// pause: reaching for warmer means wanting Luz on.
    fn nudge(&mut self, mireds: f64, brightness: f64) {
        if self.paused() {
            self.cfg.paused_until = 0;
            self.save();
        }
        self.replan();
        let mut a = self.adjust.unwrap_or(Adjust {
            mireds: 0.0,
            brightness: 1.0,
            during: self.plan.phase,
        });
        // Held at the ends of the range, so pressing on past one does not
        // bank presses that then have to be undone.
        let base = 1e6 / self.plan.kelvin;
        let lo = 1e6 / MAX_K as f64 - base;
        let hi = 1e6 / MIN_K as f64 - base;
        a.mireds = (a.mireds + mireds).clamp(lo, hi);
        a.brightness = (a.brightness + brightness).clamp(DIM_FLOOR, 1.0);
        a.brightness = (a.brightness * 10.0).round() / 10.0;
        self.adjust = (a.mireds.abs() > 0.5 || a.brightness < 1.0).then_some(a);
        self.follow(QUICK);
        self.paint_icon(false);
        self.rebuild_menu(false);
    }

    /// Dimmer (-1) or brighter (1): the monitors' own backlights where they
    /// answer, which really turns the light down; the gamma ramp where none
    /// does.
    fn dim(&mut self, dir: i32) {
        if self.backlight.monitors().is_empty() {
            self.nudge(0.0, dir as f64 * DIM_STEP);
        } else {
            self.backlight.step(dir * BACKLIGHT_STEP);
        }
    }

    fn next_morning(&self) -> Option<(Event, f64, f64)> {
        schedule::next_morning(self.now.minute, self.sun.day, &self.colours())
    }

    fn check_rival(&mut self) {
        self.rival_checked = Instant::now();
        if rival::flux_running() {
            self.rival_mentioned = true;
            lwarn!("f.lux is running too");
            notify::show(
                "f.lux is running too",
                "Quit f.lux, so the two don't take turns setting your screen's colour.",
                None,
            );
        }
    }

    fn save(&mut self) {
        match self.cfg.save() {
            Ok(()) => self.cfg_mtime = Config::modified(),
            Err(e) => lwarn!("could not save config: {e}"),
        }
    }

    fn save_and_follow(&mut self) {
        self.save();
        self.follow(GENTLE);
        self.paint_icon(false);
    }

    fn reload_if_edited(&mut self) {
        let mtime = Config::modified();
        if mtime.is_none() || mtime == self.cfg_mtime {
            return;
        }
        self.cfg_mtime = mtime;
        let Some(fresh) = Config::reload() else {
            return;
        };
        if fresh == self.cfg {
            return;
        }
        linfo!("config edited on disk");
        if (fresh.latitude, fresh.longitude) != (self.cfg.latitude, self.cfg.longitude) {
            self.place = Place::find(fresh.latitude, fresh.longitude);
            if let Some(p) = &self.place {
                linfo!("location: {}", p.describe());
            }
        }
        if fresh.hotkeys != self.cfg.hotkeys {
            self.hotkeys.enable(fresh.hotkeys);
        }
        self.cfg = fresh;
        self.follow(GENTLE);
        self.paint_icon(false);
        self.rebuild_menu(true);
    }

    fn face(&self) -> Face {
        if self.paused() {
            return Face::Paused;
        }
        match self.plan.phase {
            Phase::Day => Face::Day,
            Phase::Evening => Face::Evening,
            Phase::Bedtime => Face::Bedtime,
        }
    }

    fn paint_icon(&mut self, force: bool) {
        let face = self.face();
        if face == self.face && !force {
            return;
        }
        self.face = face;
        match tray_image(face, self.dark_taskbar) {
            Ok(img) => {
                #[cfg(target_os = "macos")]
                let _ = self.tray.set_icon_templated(Some(img));
                #[cfg(windows)]
                let _ = self.tray.set_icon(Some(img));
            }
            Err(e) => lerror!("{e}"),
        }
    }

    /// "Evening · 3400 K", the menu's top line.
    fn status_line(&self) -> String {
        if self.paused() {
            return "Paused".into();
        }
        let phase = match self.plan.phase {
            Phase::Day => "Daytime",
            Phase::Evening => "Evening",
            Phase::Bedtime => "Bedtime",
        };
        let k = (self.target.0 / 10.0).round() as u32 * 10;
        let mut line = format!("{phase} \u{00b7} {}", color::label(k));
        if self.target.1 < 1.0 {
            line.push_str(&format!(
                " \u{00b7} {}% bright",
                (self.target.1 * 100.0).round()
            ));
        }
        line
    }

    /// The line under it: what happens next, and when.
    fn detail_line(&self) -> String {
        if self.paused() {
            let left = self.cfg.paused_until.saturating_sub(now_unix());
            let at = self.now.minute + left as f64 / 60.0;
            return format!("Until {} ({} left)", clock_label(at), span(left));
        }
        let mut line = match (self.plan.next, self.sun.day) {
            (Some((e, at)), _) => {
                let verb = if self.plan.fading { "Fading" } else { "" };
                let next = format!("{} at {}", e.label(), clock_label(at));
                if verb.is_empty() {
                    next
                } else {
                    format!("{verb} \u{00b7} {next}")
                }
            }
            (None, Day::AlwaysUp) => "The sun doesn't set today".into(),
            (None, _) => "The sun doesn't rise today".into(),
        };
        if self.adjust.is_some() {
            line.push_str(" \u{00b7} adjusted");
        }
        line
    }

    /// The top two lines and the tooltip, changed in place.
    fn update_header(&mut self) {
        let header = (self.status_line(), self.detail_line());
        if header == self.header {
            return;
        }
        if let Some((status, detail)) = &self.header_items {
            status.set_text(&header.0);
            detail.set_text(&header.1);
        }
        let _ = self
            .tray
            .set_tooltip(Some(format!("Luz \u{00b7} {}\n{}", header.0, header.1)));
        self.header = header;
    }

    fn rebuild_menu(&mut self, force: bool) {
        let shown = self.screen.as_ref().map_or(Shown::default(), Screen::shown);
        let unlocked = !shown.limited || gamma::warmer_unlocked();
        let morning = self.next_morning().map(|(_, _, at)| clock_label(at));
        let autostart_on = autostart::is_enabled();
        let sig = format!(
            "{:?}|{}|{:?}|{}{}{}{}{:?}|{:?}|{:?}|{:?}",
            Config {
                paused_until: 0,
                ..self.cfg.clone()
            },
            self.paused(),
            self.adjust.is_some(),
            shown.limited,
            shown.screens == 0,
            unlocked,
            autostart_on,
            morning,
            self.face,
            self.place.as_ref().map(Place::describe),
            self.backlight.monitors(),
        );
        if sig == self.menu_sig && !force {
            return;
        }
        if menu_is_open() {
            // Rebuilt once it closes: the signature still differs then.
            self.update_header();
            return;
        }
        self.menu_sig = sig;
        self.header = (self.status_line(), self.detail_line());
        let _ = self.tray.set_tooltip(Some(format!(
            "Luz \u{00b7} {}\n{}",
            self.header.0, self.header.1
        )));

        let menu = Menu::new();
        let face_glyph = match self.face() {
            Face::Day => "sun",
            Face::Evening => "sunset",
            Face::Bedtime => "moon",
            Face::Paused => "pause",
        };
        let status = IconMenuItem::with_id(
            ID_INERT,
            &self.header.0,
            false,
            art::glyph(face_glyph),
            None,
        );
        let detail = MenuItem::with_id(ID_INERT, &self.header.1, false, None);
        let _ = menu.append_items(&[&status, &detail]);
        self.header_items = Some((status, detail));
        self.limit_rows(&menu, shown, unlocked);
        let _ = menu.append(&PredefinedMenuItem::separator());

        if self.paused() {
            let _ = menu.append(&IconMenuItem::with_id(
                ID_RESUME,
                columns("Resume", Action::Pause.label()),
                true,
                art::glyph("play"),
                None,
            ));
        } else {
            let _ = menu.append(&IconMenuItem::with_id(
                ID_PAUSE_HOUR,
                columns("Pause for an hour", Action::Pause.label()),
                true,
                art::glyph("pause"),
                None,
            ));
            if let Some((event, _, at)) = self.next_morning() {
                let what = match event {
                    Event::Wake => "Pause until morning",
                    _ => "Pause until sunrise",
                };
                let _ = menu.append(&IconMenuItem::with_id(
                    ID_PAUSE_MORNING,
                    columns(what, &clock_label(at)),
                    true,
                    art::glyph("sunrise"),
                    None,
                ));
            }
        }
        let _ = menu.append(&PredefinedMenuItem::separator());

        for (id, text, glyph, action) in [
            (ID_WARMER, "Warmer", "flame", Action::Warmer),
            (ID_COOLER, "Cooler", "snowflake", Action::Cooler),
            (ID_DIMMER, "Dimmer", "sun-dim", Action::Dimmer),
            (ID_BRIGHTER, "Brighter", "sun-medium", Action::Brighter),
        ] {
            let _ = menu.append(&IconMenuItem::with_id(
                id,
                columns(text, action.label()),
                true,
                art::glyph(glyph),
                None,
            ));
        }
        if self.adjust.is_some() {
            let until = self
                .plan
                .next
                .map(|(e, at)| format!("{} {}", e.label().to_lowercase(), clock_label(at)))
                .unwrap_or_default();
            let _ = menu.append(&IconMenuItem::with_id(
                ID_UNDO,
                columns("Back to the schedule", &format!("held until {until}")),
                true,
                art::glyph("rotate-ccw"),
                None,
            ));
        }
        if let Some(brightness) = backlight_menu(&self.backlight.monitors()) {
            let _ = menu.append(&brightness);
        }
        let _ = menu.append(&PredefinedMenuItem::separator());

        let colours = Submenu::new("Colours", true);
        colours.set_icon(art::glyph("palette"));
        let _ = colours.append_items(&[
            &kelvin_menu("Daytime", ID_DAY_K, self.cfg.day_kelvin, &DAY_PRESETS),
            &kelvin_menu(
                "Sunset",
                ID_SUNSET_K,
                self.cfg.sunset_kelvin,
                &SUNSET_PRESETS,
            ),
            &kelvin_menu(
                "Bedtime",
                ID_BEDTIME_K,
                self.cfg.bedtime_kelvin,
                &BEDTIME_PRESETS,
            ),
        ]);
        let _ = menu.append(&colours);

        let settings = self.settings_menu(autostart_on);
        let _ = menu.append(&settings);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&IconMenuItem::with_id(
            ID_UPDATES,
            "Check for updates\u{2026}",
            true,
            art::glyph("refresh-cw"),
            None,
        ));
        let _ = menu.append(&IconMenuItem::with_id(
            ID_QUIT,
            "Quit Luz",
            true,
            art::glyph("power"),
            shortcut('q'),
        ));

        self.tray.set_menu(Some(Box::new(menu)));
    }

    /// When Windows won't let the screen go as warm as asked: why, and the
    /// way round it.
    fn limit_rows(&self, menu: &Menu, shown: Shown, unlocked: bool) {
        if shown.limited {
            let k = (shown.kelvin / 100.0).round() as u32 * 100;
            let _ = menu.append(&MenuItem::with_id(
                ID_INERT,
                format!("Windows is holding it at about {}", color::label(k)),
                false,
                None,
            ));
            if unlocked {
                let _ = menu.append(&MenuItem::with_id(
                    ID_INERT,
                    "Restart Windows to go warmer",
                    false,
                    None,
                ));
            } else {
                let _ = menu.append(&IconMenuItem::with_id(
                    ID_UNLOCK,
                    "Allow warmer colours\u{2026}",
                    true,
                    art::glyph("lock-open"),
                    None,
                ));
            }
        } else if shown.screens == 0 && !self.paused() {
            let _ = menu.append(&MenuItem::with_id(
                ID_INERT,
                "No screen is taking Luz's colours",
                false,
                None,
            ));
        }
    }

    fn settings_menu(&self, autostart_on: bool) -> Submenu {
        let settings = Submenu::new("Settings", true);
        settings.set_icon(art::glyph("settings"));

        let place = match &self.place {
            Some(p) => p.describe(),
            None => "Location unknown: set it in the settings file".into(),
        };
        let place = IconMenuItem::with_id(ID_INERT, place, false, art::glyph("map-pin"), None);

        let bed = Submenu::new(
            columns(
                "Bedtime mode",
                if self.cfg.bedtime {
                    &self.cfg.bedtime_at
                } else {
                    "off"
                },
            ),
            true,
        );
        bed.set_icon(art::glyph("bed"));
        let _ = bed.append(&CheckMenuItem::with_id(
            ID_BEDTIME,
            "Bedtime mode",
            true,
            self.cfg.bedtime,
            None,
        ));
        let _ = bed.append(&PredefinedMenuItem::separator());
        for t in with_current(&BED_TIMES, &self.cfg.bedtime_at) {
            let _ = bed.append(&CheckMenuItem::with_id(
                format!("{ID_BED_AT}{t}"),
                &t,
                true,
                self.cfg.bedtime && t == self.cfg.bedtime_at,
                None,
            ));
        }

        let wake = Submenu::new(columns("Wake time", &self.cfg.wake_at), true);
        wake.set_icon(art::glyph("alarm-clock"));
        for t in with_current(&WAKE_TIMES, &self.cfg.wake_at) {
            let _ = wake.append(&CheckMenuItem::with_id(
                format!("{ID_WAKE_AT}{t}"),
                &t,
                true,
                t == self.cfg.wake_at,
                None,
            ));
        }

        let fade = Submenu::new("Transitions", true);
        let custom = self.cfg.fade_seconds != SLOW_FADE && self.cfg.fade_seconds != FAST_FADE;
        let _ = fade.append_items(&[
            &CheckMenuItem::with_id(
                format!("{ID_FADE}{SLOW_FADE}"),
                "Slow (1 hour)",
                true,
                self.cfg.fade_seconds == SLOW_FADE,
                None,
            ),
            &CheckMenuItem::with_id(
                format!("{ID_FADE}{FAST_FADE}"),
                "Fast (20 seconds)",
                true,
                self.cfg.fade_seconds == FAST_FADE,
                None,
            ),
        ]);
        if custom {
            let _ = fade.append(&CheckMenuItem::with_id(
                ID_INERT,
                format!("Yours ({})", span(self.cfg.fade_seconds as u64)),
                false,
                true,
                None,
            ));
        }

        let keys = CheckMenuItem::with_id(
            ID_HOTKEYS,
            "Keyboard shortcuts",
            true,
            self.cfg.hotkeys,
            None,
        );
        let login = CheckMenuItem::with_id(ID_AUTOSTART, "Run at login", true, autostart_on, None);
        let edit = IconMenuItem::with_id(
            ID_EDIT,
            "Edit settings file\u{2026}",
            true,
            art::glyph("file-cog"),
            shortcut(','),
        );
        let version = MenuItem::with_id(
            ID_INERT,
            concat!("Luz ", env!("CARGO_PKG_VERSION")),
            false,
            None,
        );
        let sep = PredefinedMenuItem::separator();
        let sep2 = PredefinedMenuItem::separator();
        let sep3 = PredefinedMenuItem::separator();
        let items: [&dyn IsMenuItem; 11] = [
            &place, &sep, &bed, &wake, &fade, &keys, &login, &sep2, &edit, &sep3, &version,
        ];
        let _ = settings.append_items(&items);
        settings
    }
}

/// "Brightness  30% ▸", for the monitors whose backlight answers: their levels
/// straight under it for one, a submenu each for several. `None` for none.
fn backlight_menu(monitors: &[Monitor]) -> Option<Submenu> {
    let levels = |sub: &Submenu, i: usize, now: u8| {
        let mut all = BACKLIGHT_PRESETS.to_vec();
        if !all.contains(&now) {
            all.push(now);
            all.sort_unstable_by(|a, b| b.cmp(a));
        }
        for level in all {
            let _ = sub.append(&CheckMenuItem::with_id(
                format!("{ID_BACKLIGHT}{i}:{level}"),
                format!("{level}%"),
                true,
                level == now,
                None,
            ));
        }
    };
    let menu = match monitors {
        [] => return None,
        [only] => {
            let sub = Submenu::new(columns("Brightness", &format!("{}%", only.level)), true);
            levels(&sub, 0, only.level);
            sub
        }
        several => {
            let sub = Submenu::new("Brightness", true);
            for (i, m) in several.iter().enumerate() {
                let each = Submenu::new(columns(&m.name, &format!("{}%", m.level)), true);
                levels(&each, i, m.level);
                let _ = sub.append(&each);
            }
            sub
        }
    };
    menu.set_icon(art::glyph("sun-medium"));
    Some(menu)
}

/// "Sunset  3400 K ▸", and the presets under it, with the current one ticked.
fn kelvin_menu(title: &str, prefix: &str, current: u32, presets: &[u32]) -> Submenu {
    let sub = Submenu::new(columns(title, &color::label(current)), true);
    let mut all: Vec<u32> = presets.to_vec();
    if !all.contains(&current) {
        all.push(current);
        all.sort_unstable_by(|a, b| b.cmp(a));
    }
    for k in all {
        let name = color::name(k).unwrap_or("Yours");
        let _ = sub.append(&CheckMenuItem::with_id(
            format!("{prefix}{k}"),
            columns(name, &color::label(k)),
            true,
            k == current,
            None,
        ));
    }
    sub
}

/// The menu's choices, plus the config's own value where it isn't one of
/// them, in clock order from the evening.
fn with_current(choices: &[&str], current: &str) -> Vec<String> {
    let mut all: Vec<String> = choices.iter().map(|s| s.to_string()).collect();
    if !all.iter().any(|c| c == current) {
        all.push(current.to_string());
        // Hours after noon first, so 23:30 sorts before 00:30.
        let key =
            |t: &String| crate::util::parse_clock(t).map_or(0, |m| (m + 1440 - 12 * 60) % 1440);
        all.sort_by_key(key);
    }
    all
}

fn parse<T: std::str::FromStr>(id: &str, prefix: &str) -> Option<T> {
    id.strip_prefix(prefix)?.parse().ok()
}

/// Left text, with right text in a column against the menu's edge. Windows
/// menus right-align whatever follows a tab, the way they show shortcuts;
/// macOS ones don't, so there the two are just spaced apart.
fn columns(left: &str, right: &str) -> String {
    if right.is_empty() {
        left.to_string()
    } else if cfg!(windows) {
        format!("{left}\t{right}")
    } else {
        format!("{left}    {right}")
    }
}

/// Whether a menu is open right now. On macOS the run loop is in event
/// tracking mode for exactly as long as a menu is being shown.
#[cfg(target_os = "macos")]
fn menu_is_open() -> bool {
    use objc2_app_kit::NSEventTrackingRunLoopMode;
    use objc2_foundation::NSRunLoop;
    let mode = NSRunLoop::currentRunLoop().currentMode();
    mode.is_some_and(|m| *m == *unsafe { NSEventTrackingRunLoopMode })
}

/// Windows has no such signal. Its tray menu runs a modal loop of its own,
/// and replacing the menu there doesn't close it.
#[cfg(windows)]
fn menu_is_open() -> bool {
    false
}

/// ⌘ and a key, on macOS. Windows tray menus can't take keyboard shortcuts.
fn shortcut(key: char) -> Option<Accelerator> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    use tray_icon::menu::accelerator::{Code, Modifiers};
    let code = match key {
        'q' => Code::KeyQ,
        ',' => Code::Comma,
        _ => return None,
    };
    Some(Accelerator::new(Modifiers::META, code))
}

/// Ask for an administrator to lift Windows' limit on ramps: Luz runs itself
/// again, elevated, as `luz unlock`.
#[cfg(windows)]
fn unlock_warmer() {
    match crate::elevate("unlock") {
        Ok(()) => notify::show(
            "Warmer colours are allowed",
            "Restart Windows, and Luz can go as warm as you like.",
            None,
        ),
        Err(e) => lwarn!("not unlocked: {e}"),
    }
}

#[cfg(target_os = "macos")]
fn unlock_warmer() {}

/// Open config.json in a text editor, creating it first if need be.
fn open_config() {
    let Some(path) = Config::path() else { return };
    if !path.exists() {
        let _ = Config::load().save();
    }
    #[cfg(windows)]
    let spawned = std::process::Command::new("notepad.exe").arg(&path).spawn();
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("/usr/bin/open")
        .arg("-t")
        .arg(&path)
        .spawn();
    if let Err(e) = spawned {
        lwarn!("could not open {}: {e}", path.display());
    }
}

/// Open Luz's web page in the browser, which compares the version it's given
/// with the latest release. Luz itself never goes on the network.
fn check_for_updates() {
    let url = concat!("https://josebarbosa.dev/luz/?v=", env!("CARGO_PKG_VERSION"));
    #[cfg(windows)]
    let opened = {
        use crate::util::{pcwstr, wide};
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let (verb, file) = (wide("open"), wide(url));
        let rc = unsafe {
            ShellExecuteW(
                None,
                pcwstr(&verb),
                pcwstr(&file),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // Above 32 is success, as for elevate().
        if rc.0 as isize > 32 {
            Ok(())
        } else {
            Err(format!("error {}", rc.0 as isize))
        }
    };
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string());
    if let Err(e) = opened {
        lwarn!("could not open {url}: {e}");
    }
}

fn tray_image(face: Face, dark: bool) -> Result<Icon, String> {
    Icon::from_rgba(icon::render(face, dark, icon::EDGE), icon::EDGE, icon::EDGE)
        .map_err(|e| format!("could not build the tray image: {e}"))
}

fn forward_menu_events(proxy: &EventLoopProxy<UserEvent>) {
    let proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = proxy.send_event(UserEvent::Menu(e.id.0));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odd_times_join_the_list_in_evening_order() {
        let t = with_current(&["22:00", "23:00", "00:00"], "23:30");
        assert_eq!(t, ["22:00", "23:00", "23:30", "00:00"]);
        assert_eq!(with_current(&["07:00"], "07:00"), ["07:00"]);
    }
}
