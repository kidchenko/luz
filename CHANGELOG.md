# Changelog

## Unreleased

- Linux. On X11, and on Hyprland, Sway and other wlroots compositors, Luz
  sets the gamma ramps as on Windows and macOS. On GNOME and KDE with
  Wayland it drives the desktop's own Night Light, and puts your Night Light
  settings back when it quits. Run at login, notifications and the keyboard
  shortcuts (X11) work too, and Redshift, Gammastep or wlsunset running
  alongside gets the same word f.lux does. Releases carry `luz-x86_64-linux.tar.gz`.

- A tidier menu. Warmer, Cooler, Dimmer and Brighter are under **Adjust**,
  there are icons only on the status line, Pause and Quit, and **Check for
  updates…** is in Settings under the version.
- On macOS, icons and checkmarks share one column, so every label lines up,
  the icons follow the menu's light or dark look, and the keyboard shortcuts
  sit in the menu's own shortcut column like ⌘Q, shown only while they're on.
- **Check for updates…** in Settings opens Luz's web page with your version,
  and the page says whether a newer one is out. Luz still doesn't use the
  network itself.

## [0.0.1] - 2026-10-23

First release.

- A sun in the tray (Windows) or menu bar (macOS) that turns your screen
  warmer after sunset and back to daylight in the morning, like f.lux.
- Sunrise and sunset for where you are, guessed from your time zone's city,
  with no location prompt and no network. Or give it your coordinates with
  `luz location`.
- Daytime, Sunset and Bedtime colours from f.lux's list, down to 1200 K, or
  any temperature in the config. Slow (1 hour) or fast (20 second) fades, in
  mireds so they look even.
- Bedtime mode: warmer still from bedtime until your wake time, and mornings
  that wait for you if the sun is up first.
- Pause for an hour or until sunrise. Warmer, cooler, dimmer and brighter, on
  top of the schedule until it next changes.
- Monitor brightness over DDC/CI on Windows: a **Brightness** menu for each
  monitor that answers, and Dimmer/Brighter (Alt+PgDn/PgUp) turn the real
  backlight down and up. Without one, they darken the picture instead.
- f.lux's keyboard shortcuts: Alt+End, Alt+PgUp/PgDn, Alt+Shift+PgUp/PgDn
  (⌥ on a Mac).
- Keeps a screen's calibration curve, tinting it rather than replacing it,
  and puts it back on quit.
- When Windows won't go as warm as asked, Luz goes as far as it allows, says
  so in the menu, and offers to lift the limit. The installer lifts it too.
- Tells you once if f.lux is running at the same time.
- Opening Luz again pauses or resumes it, on both systems.
- `luz status`, `luz location` and `luz reset` from a terminal.
- Run at login. On macOS 13+ it's a regular login item in System Settings.
- Windows installer, Chocolatey and winget manifests, a universal macOS DMG
  (Apple silicon and Intel), and a Homebrew cask in kidchenko/homebrew-tap.
