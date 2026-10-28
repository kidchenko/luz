// No console window: this is a tray app. No-op off Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! Luz ("light") warms the screen after sunset and gives it back in the
//! morning, from the tray, like f.lux, on both Windows and macOS.
//!
//! | module            | responsibility                                       |
//! |-------------------|------------------------------------------------------|
//! | `schedule`        | the colour for a minute of the day                   |
//! | `sun`             | sunrise and sunset                                   |
//! | `place`           | where the computer is, from config or the time zone  |
//! | `zones`           | time zones' cities and coordinates (generated)       |
//! | `color`           | colour temperature to red, green and blue            |
//! | `gamma`           | the screens' ramps                                   |
//! | `screen`          | the thread that fades and keeps the ramps set        |
//! | `backlight`       | monitor brightness over DDC/CI                       |
//! | `hotkeys`         | f.lux's shortcuts                                    |
//! | `rival`           | whether f.lux is running too                         |
//! | `config`          | the persisted preferences                            |
//! | `icon`            | draws the sun                                        |
//! | `art`             | the menu's glyphs, from Lucide                       |
//! | `notify`          | notifications                                        |
//! | `app`             | the tray icon and its menu                           |
//! | `autostart`       | run at login                                         |
//!
//! The log, the config file's home, single instance and the taskbar's theme
//! come from `traykit`, shared with the other kidchenko tray apps. A second
//! launch pauses or resumes the running copy.

mod app;
mod art;
mod autostart;
mod backlight;
mod color;
mod config;
mod gamma;
mod hotkeys;
mod icon;
mod notify;
mod place;
mod rival;
mod schedule;
mod screen;
mod sun;
mod util;
mod zones;

use app::{App, Flow, UserEvent};
use config::Config;
use place::Place;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use traykit::{lerror, linfo};
use util::clock_label;

const USAGE: &str = "\
Luz: warm your screen at night, like f.lux.

usage:
  luz                        run in the tray (or pause/resume the running copy)
  luz status                 the colour now, and the day's sun
  luz location <lat> <lon>   set where you are, in degrees (south and west negative)
  luz location auto          go back to guessing from the time zone
  luz reset                  put the screens' colours back (after a crash)";

fn main() {
    // Names the config folder, the log, the socket and the login entry, and
    // opens the log; everything in traykit needs it first.
    traykit::init(traykit::App {
        name: "Luz",
        id: "luz",
        bundle_id: "com.kidchenko.luz",
        summary: "Warm your screen at night",
    });
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => tray(),
        Some("--dump-icons") => {
            let dir = args.get(1).map(String::as_str).unwrap_or("target/icons");
            std::process::exit(match icon::dump(dir) {
                Ok(()) => 0,
                Err(_) => 1,
            });
        }
        Some(_) => {
            traykit::util::attach_console();
            std::process::exit(cli(&args));
        }
    }
}

fn cli(args: &[String]) -> i32 {
    match args[0].as_str() {
        "status" => {
            print!("{}", status(&Config::load()));
            0
        }
        "location" => location(&args[1..]),
        "reset" => {
            gamma::restore();
            println!("Screens put back to their own colours.");
            println!("(A running Luz sets them again within a few seconds; quit it first.)");
            0
        }
        // What "Allow warmer colours…" runs, elevated. Windows only.
        "unlock" => match gamma::unlock_warmer() {
            Ok(()) => {
                linfo!("warmer colours unlocked");
                println!("Done. Restart Windows, and Luz can go as warm as you like.");
                0
            }
            Err(e) => {
                lerror!("{e}");
                eprintln!("luz: {e} (this needs an administrator)");
                1
            }
        },
        "--version" | "-V" => {
            println!("luz {}", env!("CARGO_PKG_VERSION"));
            0
        }
        "--help" | "-h" | "help" => {
            println!("{USAGE}");
            0
        }
        other => {
            eprintln!("luz: unknown command '{other}'\n\n{USAGE}");
            2
        }
    }
}

/// `luz location`: written to the config, which a running Luz picks up.
fn location(args: &[String]) -> i32 {
    let mut cfg = Config::load();
    match args {
        [auto] if auto == "auto" => {
            cfg.latitude = None;
            cfg.longitude = None;
        }
        [lat, lon] => {
            let (Ok(lat), Ok(lon)) = (lat.parse::<f64>(), lon.parse::<f64>()) else {
                eprintln!("luz: location needs two numbers, like `luz location -23.55 -46.63`");
                return 2;
            };
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                eprintln!("luz: latitude goes from -90 to 90, longitude from -180 to 180");
                return 2;
            }
            cfg.latitude = Some(lat);
            cfg.longitude = Some(lon);
        }
        [] => {
            match Place::find(cfg.latitude, cfg.longitude) {
                Some(p) => println!("{}", p.describe()),
                None => println!("Unknown. Set it with `luz location <lat> <lon>`."),
            }
            return 0;
        }
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    }
    if let Err(e) = cfg.save() {
        eprintln!("luz: could not save the config: {e}");
        return 1;
    }
    match Place::find(cfg.latitude, cfg.longitude) {
        Some(p) => println!("Location: {}", p.describe()),
        None => println!("Location: unknown"),
    }
    0
}

/// What `luz status` prints.
fn status(cfg: &Config) -> String {
    let now = util::local_now();
    let place = Place::find(cfg.latitude, cfg.longitude);
    let day = match &place {
        Some(p) => sun::day(
            p.lat,
            p.lon,
            sun::day_of_year(now.year, now.month, now.day),
            now.utc_offset,
        ),
        None => sun::Day::Normal {
            rise: 7.0 * 60.0,
            set: 19.0 * 60.0,
        },
    };
    let colours = schedule::Colours {
        day: cfg.day_kelvin as f64,
        sunset: cfg.sunset_kelvin as f64,
        bedtime: cfg.bedtime_kelvin as f64,
        bed: cfg
            .bedtime
            .then(|| (cfg.bedtime_minute() as f64, cfg.wake_minute() as f64)),
        fade: cfg.fade_seconds as f64 / 60.0,
    };
    let plan = schedule::plan(now.minute, day, &colours);

    let mut out = String::new();
    let mut row = |k: &str, v: String| out.push_str(&format!("{k:<10}{v}\n"));
    let paused = cfg.paused_until > util::now_unix();
    row(
        "now",
        if paused {
            "paused".into()
        } else {
            format!("{:?}, {:.0} K", plan.phase, plan.kelvin)
        },
    );
    if let Some((e, at)) = plan.next {
        row("next", format!("{} at {}", e.label(), clock_label(at)));
    }
    row(
        "sun",
        match day {
            sun::Day::Normal { rise, set } => {
                format!("up {}, down {}", clock_label(rise), clock_label(set))
            }
            sun::Day::AlwaysUp => "up all day".into(),
            sun::Day::AlwaysDown => "down all day".into(),
        },
    );
    row(
        "location",
        place.map_or("unknown".into(), |p| {
            if p.guessed {
                format!("{} ({})", p.describe(), place::coordinates(p.lat, p.lon))
            } else {
                p.describe()
            }
        }),
    );
    row(
        "colours",
        format!(
            "daytime {} K, sunset {} K, bedtime {} K",
            cfg.day_kelvin, cfg.sunset_kelvin, cfg.bedtime_kelvin
        ),
    );
    row(
        "bedtime",
        if cfg.bedtime {
            format!("{} to {}", cfg.bedtime_at, cfg.wake_at)
        } else {
            "off".into()
        },
    );
    out
}

/// Run this executable again as an administrator with `arg`. Windows asks
/// the user first.
#[cfg(windows)]
pub fn elevate(arg: &str) -> Result<(), String> {
    use traykit::util::{pcwstr, wide};
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let (verb, file, params) = (wide("runas"), wide(&exe.to_string_lossy()), wide(arg));
    let rc = unsafe {
        ShellExecuteW(
            None,
            pcwstr(&verb),
            pcwstr(&file),
            pcwstr(&params),
            PCWSTR::null(),
            SW_HIDE,
        )
    };
    // Above 32 is success; anything else is an error code, 5 when the user
    // said no.
    if rc.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("elevation refused or failed ({})", rc.0 as isize))
    }
}

fn tray() {
    linfo!(
        "--- Luz {} starting (pid {}) ---",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );

    // Before GTK starts its threads, which would otherwise die of them.
    #[cfg(target_os = "linux")]
    let signals = block_quit_signals();

    // A second launch pauses or resumes the running instance and exits.
    let Some(signal) = traykit::single_instance::acquire() else {
        linfo!("another instance owns the tray; asked it to pause or resume, exiting");
        return;
    };

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    // Menu-bar agent, no Dock icon. tao resets the activation policy as it
    // starts, overriding LSUIElement, so it has to be asked for here too.
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        event_loop.set_activate_ignoring_other_apps(false);
    }
    let proxy = event_loop.create_proxy();

    // The loop's own timer can be lost while a menu is open on macOS (tao
    // doesn't wake in the menu's run loop mode). A thread's nudge every second
    // always gets through.
    let tick_proxy = proxy.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if tick_proxy.send_event(UserEvent::Tick).is_err() {
            return;
        }
    });

    #[cfg(target_os = "linux")]
    quit_on_signals(signals, proxy.clone());

    let summon_proxy = proxy.clone();
    signal.watch(move || {
        let _ = summon_proxy.send_event(UserEvent::Summon);
    });

    // The tray is built once the loop is running: on macOS the status item
    // cannot be created before the application has finished launching.
    let mut app: Option<App> = None;

    event_loop.run(move |event, _target, control_flow| {
        match event {
            Event::NewEvents(StartCause::Init) => {
                #[cfg(target_os = "macos")]
                drop_launch_activation();
                match App::new(proxy.clone()) {
                    Ok(a) => app = Some(a),
                    Err(e) => {
                        lerror!("{e}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                }
            }
            Event::UserEvent(e) => {
                if let Some(a) = app.as_mut() {
                    if a.handle(e) == Flow::Exit {
                        // Dropping the app stops the screen thread, which
                        // puts the screens back before the process goes.
                        app = None;
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                }
            }
            // macOS: opening Luz.app again (Finder, Spotlight, `open`) does not
            // start a second process; LaunchServices sends the running one a
            // reopen instead. Treat it like any other relaunch.
            Event::Reopen { .. } => {
                if let Some(a) = app.as_mut() {
                    linfo!("reopened by LaunchServices");
                    let _ = a.handle(UserEvent::Summon);
                }
            }
            _ => {}
        }
        if app.is_some() {
            *control_flow = ControlFlow::Wait;
        }
    });
}

/// The signals that end a session or a terminal's Luz. X11 keeps a ramp after
/// the process that set it is gone, so they must quit the way the menu does,
/// putting the screens back. Blocked in every thread, and waited for in one.
#[cfg(target_os = "linux")]
fn block_quit_signals() -> libc::sigset_t {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::sigaddset(&mut set, s);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

#[cfg(target_os = "linux")]
fn quit_on_signals(set: libc::sigset_t, proxy: tao::event_loop::EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let mut sig = 0;
        if unsafe { libc::sigwait(&set, &mut sig) } == 0 {
            let _ = proxy.send_event(UserEvent::Signal);
        }
    });
}

/// tao asks macOS to activate the app as it finishes launching. A menu bar
/// app has nothing to bring forward, and on macOS 26 the request stays
/// pending until the first click on the menu bar icon, then completes while
/// the menu is open and closes it. Withdraw it before anything is shown.
#[cfg(target_os = "macos")]
fn drop_launch_activation() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).deactivate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_well() {
        let s = status(&Config::default());
        for key in ["now", "sun", "location", "colours", "bedtime"] {
            assert!(s.lines().any(|l| l.starts_with(key)), "{key} in\n{s}");
        }
    }
}
