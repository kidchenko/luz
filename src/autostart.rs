//! Run-at-login.
//!
//! On Windows this is the per-user Run key. On macOS 13+ it is `SMAppService`,
//! the API behind System Settings > General > Login Items, so Luz shows up
//! there by name and the user can switch it off from either side; macOS 11 and
//! 12 fall back to a LaunchAgent plist. All of them need no elevation and are
//! trivially inspectable (Task Manager's Startup tab / Login Items).

pub use imp::{is_enabled, set};

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod imp {
    use crate::util::{exe_path_quoted, pcwstr, wide};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "Luz";

    /// Closes the key on drop, so early returns cannot leak a handle.
    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = RegCloseKey(self.0);
                }
            }
        }
    }

    fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<Key> {
        let sub = wide(RUN_KEY);
        let mut hkey = HKEY::default();
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, pcwstr(&sub), None, access, &mut hkey) };
        (rc == ERROR_SUCCESS).then_some(Key(hkey))
    }

    /// Is the Run value present at all? We deliberately do not compare it against
    /// the current executable path: if the user moved the binary we would rather
    /// report "on" and let `set(true)` rewrite the path than silently show
    /// "off".
    pub fn is_enabled() -> bool {
        let Some(key) = open(KEY_QUERY_VALUE) else {
            return false;
        };
        let name = wide(VALUE_NAME);
        let rc = unsafe { RegQueryValueExW(key.0, pcwstr(&name), None, None, None, None) };
        rc == ERROR_SUCCESS
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let Some(key) = open(KEY_SET_VALUE) else {
            return Err("could not open the HKCU Run key".into());
        };
        let name = wide(VALUE_NAME);

        if !enabled {
            let rc = unsafe { RegDeleteValueW(key.0, pcwstr(&name)) };
            // Deleting something already absent is the desired end state.
            return if rc == ERROR_SUCCESS || rc.0 == 2 {
                Ok(())
            } else {
                Err(format!(
                    "could not remove the autostart entry (error {})",
                    rc.0
                ))
            };
        }

        let Some(cmd) = exe_path_quoted() else {
            return Err("could not resolve the executable path".into());
        };
        let value = wide(&cmd);
        // REG_SZ length is counted in bytes and must include the terminator.
        let bytes =
            unsafe { std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2) };
        let rc = unsafe { RegSetValueExW(key.0, pcwstr(&name), None, REG_SZ, Some(bytes)) };
        if rc == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!(
                "could not write the autostart entry (error {})",
                rc.0
            ))
        }
    }
}

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
