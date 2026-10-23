# Changelog

## [Unreleased]

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
