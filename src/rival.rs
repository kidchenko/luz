//! Whether f.lux is running too. Both set the same ramps, several times a
//! minute each, and the screen flickers between the two. Someone moving to
//! Luz has f.lux installed, so it is worth a word the first time it happens.

pub use imp::flux_running;

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    pub fn flux_running() -> bool {
        let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return false;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        let mut ok = unsafe { Process32FirstW(snap, &mut entry) }.is_ok();
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case("flux.exe") {
                found = true;
                break;
            }
            ok = unsafe { Process32NextW(snap, &mut entry) }.is_ok();
        }
        unsafe {
            let _ = CloseHandle(snap);
        }
        found
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2_app_kit::NSWorkspace;

    const FLUX: &str = "org.herf.Flux";

    pub fn flux_running() -> bool {
        let apps = NSWorkspace::sharedWorkspace().runningApplications();
        apps.iter().any(|app| {
            app.bundleIdentifier()
                .is_some_and(|id| id.to_string() == FLUX)
        })
    }
}
