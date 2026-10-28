//! Run-at-login.
//!
//! On macOS 13+ it is `SMAppService`, the API behind System Settings >
//! General > Login Items, so Luz shows up there by name and the user can
//! switch it off from either side; macOS 11 and 12 fall back to a LaunchAgent
//! plist. Elsewhere it is traykit's: the per-user Run key on Windows, and a
//! desktop entry in `~/.config/autostart` on Linux. None of them need
//! elevation, and all are easy to inspect (Login Items / Task Manager's
//! Startup tab / the desktop's startup apps).

#[cfg(target_os = "macos")]
pub use imp::{is_enabled, set};
#[cfg(not(target_os = "macos"))]
pub use traykit::autostart::{is_enabled, set};

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod imp {
    pub fn is_enabled() -> bool {
        if objc2::available!(macos = 13.0) {
            login_item::is_enabled()
        } else {
            launch_agent::is_enabled()
        }
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let exe = std::env::current_exe()
            .map_err(|e| format!("could not resolve the executable path: {e}"))?;
        let exe = exe.to_string_lossy();

        // Run straight from the DMG, or from a quarantined download, the path is
        // a mounted volume or a randomised App Translocation copy that is gone
        // by the next login. Registering it would fail silently later.
        if enabled && (exe.starts_with("/Volumes/") || exe.contains("/AppTranslocation/")) {
            return Err(format!(
                "not enabling run at login from {exe}: move Luz.app to Applications first"
            ));
        }

        if objc2::available!(macos = 13.0) {
            // A plist left by a macOS 12 install, before an OS upgrade, would
            // launch a second copy at login next to the login item.
            let _ = launch_agent::set(false, &exe);
            login_item::set(enabled)
        } else {
            launch_agent::set(enabled, &exe)
        }
    }

    /// macOS 13+: register the app bundle itself as a login item.
    mod login_item {
        use objc2_service_management::{SMAppService, SMAppServiceStatus};

        /// Only `Enabled` counts. `RequiresApproval` means the user switched
        /// Luz off in Login Items (or has yet to allow it), and it will not
        /// run at login, so the menu should not claim otherwise.
        pub fn is_enabled() -> bool {
            unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
        }

        pub fn set(enabled: bool) -> Result<(), String> {
            let service = unsafe { SMAppService::mainAppService() };
            let status = unsafe { service.status() };

            if !enabled {
                if status == SMAppServiceStatus::NotRegistered {
                    return Ok(());
                }
                return unsafe { service.unregisterAndReturnError() }.map_err(|e| {
                    format!(
                        "could not remove the login item: {}",
                        e.localizedDescription()
                    )
                });
            }

            // Registered, but turned off in System Settings. Registering again
            // cannot override the user's choice there, so take them to it.
            if status == SMAppServiceStatus::RequiresApproval {
                unsafe { SMAppService::openSystemSettingsLoginItems() };
                return Ok(());
            }
            unsafe { service.registerAndReturnError() }.map_err(|e| {
                format!("could not add the login item: {}", e.localizedDescription())
            })?;
            if unsafe { service.status() } == SMAppServiceStatus::RequiresApproval {
                unsafe { SMAppService::openSystemSettingsLoginItems() };
            }
            Ok(())
        }
    }

    /// macOS 11 and 12: a LaunchAgent plist.
    mod launch_agent {
        use std::path::PathBuf;

        const LABEL: &str = "com.kidchenko.luz";

        fn plist_path() -> Option<PathBuf> {
            std::env::var_os("HOME").map(|h| {
                PathBuf::from(h)
                    .join("Library/LaunchAgents")
                    .join(format!("{LABEL}.plist"))
            })
        }

        /// Presence of the plist is the switch. As on Windows, we do not verify
        /// the path inside it points at this binary: report "on" and let
        /// `set(true)` rewrite it rather than silently showing "off" after a
        /// move.
        pub fn is_enabled() -> bool {
            plist_path().map(|p| p.exists()).unwrap_or(false)
        }

        pub fn set(enabled: bool, exe: &str) -> Result<(), String> {
            let path = plist_path().ok_or("no HOME in environment")?;

            if !enabled {
                return match std::fs::remove_file(&path) {
                    Ok(()) => Ok(()),
                    // Already gone is the desired end state.
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(format!("could not remove the LaunchAgent: {e}")),
                };
            }

            // Deliberately not `launchctl load`ed here: RunAtLoad would
            // immediately spawn a second copy on top of the one writing this
            // file. launchd picks the agent up at the next login, which is
            // exactly when it is wanted.
            let plist = plist_xml(exe);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("could not create LaunchAgents dir: {e}"))?;
            }
            std::fs::write(&path, plist)
                .map_err(|e| format!("could not write the LaunchAgent: {e}"))
        }

        fn plist_xml(exe: &str) -> String {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
</dict>
</plist>
"#,
                exe = xml_escape(exe)
            )
        }

        fn xml_escape(s: &str) -> String {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        }
    }
}
