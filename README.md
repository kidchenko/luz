<div align="center">

<img src="assets/icon-256.png" width="112" height="112" alt="Luz">

# Luz

Warm your screen at night, from the tray.<br>
Like f.lux, in one small app, on Windows and macOS.

[Website](https://josebarbosa.dev/luz/) &nbsp;|&nbsp; [Download](https://github.com/kidchenko/luz/releases/latest)

<sub><i>Luz</i> is Portuguese for <i>light</i>.</sub>

</div>

Luz is a sun in the tray. At sunset it turns your screen's colour from
daylight to a warm, lamp-like light, more slowly than you'll notice, and in the
morning it turns it back. With Bedtime mode it goes warmer still before you
sleep. It works out sunrise and sunset for where you are, and it has f.lux's
colours and keyboard shortcuts, so if you're coming from f.lux nothing has to
be relearned.

It's one small executable (under 1 MB) with nothing else to install. It
doesn't ask for your location and doesn't use the network.

## The sun

| | |
|---|---|
| **Sun** | Daytime. Your screen looks the way it always does. |
| **Sun going down** | Evening, after sunset. The screen is warm. |
| **Moon** | Bedtime mode. Warmer still, until you wake. |
| **Sun struck through** | Paused. Your screen is back to its own colours for now. |

On Windows the sun and moon are filled with the colour your screen is turning,
so you can tell at a glance. Either click opens the menu.

Running `luz` again while it's already in the tray pauses it for an hour, or
resumes it if it's paused. Pin it to the taskbar, or open Luz.app again on
macOS, and that's a switch for a moment of true colour.

## The menu

The top line says where Luz is right now (**Evening · 3400 K**), and the line
under it says what comes next (**Bedtime at 23:00**). Under them:

| | |
|---|---|
| **Pause for an hour** | Back to your screen's own colours for an hour, for photos or design work. While paused, this reads **Resume**. |
| **Pause until sunrise** | Off for the rest of the night. With Bedtime mode on it's **Pause until morning**, at your wake time. |
| **Warmer, Cooler** | A step at a time, on top of the schedule. |
| **Dimmer, Brighter** | Turns your monitors' own backlight down or up, 10% at a time. Where no monitor lets Luz do that, it darkens the picture instead, down to 30%. [More below.](#brightness) |
| **Brightness** | Each monitor's backlight level, with 100, 75, 50, 25 and 0% to pick from. Only there when a monitor answers. |
| **Back to the schedule** | Drops your adjustments. They also go by themselves at the next change, so a nudge at 21:00 lasts until bedtime. |
| **Colours** | The **Daytime**, **Sunset** and **Bedtime** colours, from f.lux's list: Daylight 6500 K, Sunlight 5500 K, Fluorescent 4200 K, Halogen 3400 K, Incandescent 2700 K, Candle 1900 K, Ember 1200 K, and a few in between. |

**Settings** holds the things you set once:

| | |
|---|---|
| **Location** | Where Luz thinks you are. [More below.](#where-you-are) |
| **Bedtime mode** | On or off, and when it starts. |
| **Wake time** | When Bedtime mode ends. |
| **Transitions** | **Slow** fades over an hour, the way the light outside changes. **Fast** takes 20 seconds. |
| **Keyboard shortcuts** | f.lux's, system-wide. [More below.](#keyboard-shortcuts) |
| **Run at login** | Just for your user, no admin needed. On macOS it shows up in System Settings > General > Login Items. Move Luz.app to Applications first. |
| **Edit settings file…** | Opens the config file in a text editor (⌘, on macOS). [More below.](#configuration) |

Under that is the version you're running. **Check for updates…** opens
Luz's web page in your browser with that version, and the page asks GitHub
whether there's a newer one. Luz itself still doesn't use the network. **Quit
Luz** (⌘Q on macOS) puts your screen's colours back as it goes.

### The day

Two fades, each taking an hour (or 20 seconds on Fast):

- **At sunset**, from the Daytime colour to the Sunset colour.
- **At bedtime**, with Bedtime mode on, from there to the Bedtime colour. It
  stays that way until your wake time, then fades back.

Mornings start at sunrise. With Bedtime mode on they wait for your wake time
if it's later, so a 5 o'clock dawn in June doesn't light up your screen before
you're up. Each fade starts at its moment rather than finishing at it, as
f.lux's do. Colours fade in mireds, the scale the eye sees as even, so a fade
doesn't seem to hurry at the warm end.

Every change fades, including the ones you make. Pausing takes two seconds,
and a step warmer takes less than half a second.

### Where you are

Luz needs to know roughly where you are to know when the sun sets there. It
guesses from your time zone. Every time zone is named after a city, and
tzdata, the time zone database every OS ships, knows where that city is.
That's usually close enough. A big time zone can put you a few hundred
kilometres out, which moves sunset by a few minutes. There's no location
prompt and nothing goes over the network.

To be exact, give it your coordinates (south and west are negative):

```
luz location -23.55 -46.63     # São Paulo
luz location auto              # back to the time zone's city
```

That writes to the config file, and a Luz that's already running picks it up
within a second.

### Brightness

Luz can turn down the light itself, not just its colour. Most external
monitors take brightness commands over DDC/CI, the same channel their own
buttons use, and Luz sends them there. That's better than darkening the
picture: the backlight really dims, so there's less glare, and blacks stay
black.

Monitors that answer show up under **Brightness**, and **Dimmer** and
**Brighter** (Alt+PgDn and Alt+PgUp) move all of them 10% at a time. The level
is the monitor's own setting, so it stays where you put it, even after Luz
quits.

Laptop screens don't speak DDC/CI; their brightness keys already do this.
Neither do some docks, USB-C adapters and TVs, and some monitors have DDC/CI
switched off in their on-screen menu. With no monitor answering, Dimmer and
Brighter darken the picture with the gamma ramp instead, and that lasts until
the schedule next changes. On macOS this is the only kind for now: monitor
brightness there needs private system services, and isn't in Luz yet.

### Keyboard shortcuts

The same keys as f.lux, so your muscle memory still works:

| Windows | macOS | |
|---|---|---|
| Alt+End | ⌥End | Pause for an hour, or resume |
| Alt+Shift+PgDn | ⌥⇧⇟ | Warmer |
| Alt+Shift+PgUp | ⌥⇧⇞ | Cooler |
| Alt+PgDn | ⌥⇟ | Dimmer ([monitors' backlight](#brightness) where they answer) |
| Alt+PgUp | ⌥⇞ | Brighter |

On a Mac laptop, End and Page Up/Down are fn with the arrow keys. If another
app already has one of these (usually f.lux itself), Luz skips that one and
the rest still work.

### Coming from f.lux

Quit f.lux first, and turn off its **Start with Windows** or login item. Two
apps setting your screen's colour take turns several times a minute, and you'll
see it flicker. If f.lux is running when Luz starts, Luz tells you once.

Windows' **Night light** and macOS's **Night Shift** work differently: they
sit on top of what Luz does, so with either one on you get both. Turn them off
too.

## From a terminal

```
luz status                    # the colour now, the day's sun, and where Luz thinks you are
luz location -23.55 -46.63    # set where you are (south and west negative)
luz location auto             # back to guessing from the time zone
luz reset                     # put the screens' colours back, after a crash
luz                           # pause the running copy for an hour, or resume it
```

## Install

### Windows

```powershell
scoop bucket add kidchenko https://github.com/kidchenko/scoop-bucket
scoop install kidchenko/luz
```

winget and Chocolatey packages are on the way.

Or grab [**Luz-Setup.exe**](https://github.com/kidchenko/luz/releases/latest/download/Luz-Setup.exe)
directly. Each release also ships a portable `luz-x.y.z-x64.exe` that runs
without installing; see [Releases](https://github.com/kidchenko/luz/releases).

The installer can also do something the portable copy can't on its own: lift
Windows' limit on how warm the screen may go. [More below.](#windows-stops-at-about-3000-k)

The app isn't code-signed yet, so SmartScreen warns you the first time you run
it. Click **More info**, then **Run anyway**.

### macOS

```bash
brew install --cask kidchenko/tap/luz
```

Or download [**Luz.dmg**](https://github.com/kidchenko/luz/releases/latest/download/Luz.dmg)
(Apple silicon and Intel), open it, and drag Luz into **Applications**. It
lives in the menu bar and doesn't show up in the Dock.

The app isn't notarized yet, so macOS blocks it the first time you open it.
Homebrew takes care of that for you. Otherwise, do one of these once:

- Go to **System Settings > Privacy & Security** and click **Open Anyway**
  next to Luz.
- Run `xattr -dr com.apple.quarantine /Applications/Luz.app`

Move it into Applications before you do this. It won't work while the app is
still inside the disk image.

### Requirements

Windows 10 or later. macOS 11 or later.

## How it works

Every screen has a gamma ramp: a table the graphics card passes each red,
green and blue value through on its way to the screen. Luz writes a table that
turns blue and green down by just enough to match the colour of a light at
that temperature. Nothing is drawn over your screen, so screenshots and screen
recordings come out in their true colours.

| | |
|---|---|
| **Windows** | `SetDeviceGammaRamp`, for each screen on the desktop |
| **macOS** | `CGSetDisplayTransferByTable`, for each online display |

The colour of each temperature comes from Krystek's fit to the colour of a
glowing black body, worked out relative to 6500 K so that daylight leaves your
screen exactly as it was. Sunrise and sunset come from NOAA's sunrise
equation. Your time zone comes from the OS, and its city from a table built
from tzdata and CLDR (`assets/make_zones.py` writes `src/zones.rs`).

If a screen's ramp already has a calibration curve on it, Luz tints that curve
rather than replacing it, and puts it back when you quit. The OS drops ramps
now and then (after sleep, a resolution change or a full-screen game), so Luz
sets them again every few seconds.

The sun is drawn in code (`src/icon.rs`). The images in `assets/` and
`docs/img/` are generated from it with `.\build.ps1 -Icons`. The glyphs in the
menu come from [Lucide](https://lucide.dev) (ISC): `assets/icons/make.sh`
rasterises them into `assets/icons/glyph/`, and `assets/icons/LICENSES.md` has
the license.

## Configuration

`%APPDATA%\Luz\config.json` on Windows,
`~/Library/Application Support/Luz/config.json` on macOS. You can do
everything from the menu, but you can edit the file if you'd rather. Luz
notices changes within a second, so there's no need to restart it.

```json
{
  "dayKelvin": 6500,
  "sunsetKelvin": 3400,
  "bedtimeKelvin": 1900,
  "bedtime": true,
  "bedtimeAt": "23:00",
  "wakeAt": "07:00",
  "fadeSeconds": 3600,
  "latitude": null,
  "longitude": null,
  "hotkeys": true,
  "pausedUntil": 0
}
```

| Key | |
|---|---|
| `dayKelvin`, `sunsetKelvin`, `bedtimeKelvin` | Any temperature from 1200 to 6500, not just the ones in the menu. |
| `bedtimeAt`, `wakeAt` | 24-hour clock, local time. Any minute, not just the menu's half hours. |
| `fadeSeconds` | How long each fade takes. 1 second to 3 hours. |
| `latitude`, `longitude` | Degrees, south and west negative. Both or neither; `null` guesses from the time zone. |
| `pausedUntil` | A Unix time. Luz keeps a pause going across a restart. |

## Troubleshooting

Luz writes a log to `luz.log`, next to the config. It notes where it thinks
you are, the day's sunrise and sunset, every change of phase, and anything
the OS refused:

```
INFO  location: Bangkok, from your time zone
INFO  sun for 2026-10-23: up 06:10, down 17:55
INFO  2 screen(s) tinted
INFO  now Evening
INFO  2 screen(s) tinted, held at about 2940 K by Windows
```

`luz status` prints the same picture from a terminal.

### Windows stops at about 3000 K

Windows refuses gamma ramps that stray too far from a straight line, and on
most screens that limit falls somewhere around 3000 K, short of the Candle
colour Bedtime mode uses. When that happens Luz goes as warm as Windows
allows, and the menu says **Windows is holding it at about 2900 K**.

The installer lifts the limit (it's the **Allow colours warmer than Windows'
usual limit** box, ticked by default). With the portable copy, click **Allow
warmer colours…** in the menu and say yes to the admin prompt. Either way,
Windows only reads the setting when it starts, so restart once. It's the same
setting f.lux uses (`GdiIcmGammaRange` under
`HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ICM`), and it only allows
warmer colours. It doesn't set any.

**A monitor isn't under Brightness.** It doesn't answer DDC/CI. Look for a
DDC/CI setting in its on-screen menu, and try it on a direct cable rather
than through a dock or adapter.

**One screen isn't changing.** Windows skips the gamma ramp on screens in HDR
mode. Turn HDR off for that screen in **Settings > Display**. Some DisplayLink
and other USB displays have no ramp at all.

**The colour flickers.** Something else is setting it too, usually f.lux, a
monitor vendor's app or a game's own gamma setting. Quit the other one.

**The sunset time is wrong.** Check **Settings > Location**. If it says a city
that isn't near you, give Luz your coordinates with `luz location`.

**My screen stayed orange after a crash.** Run `luz reset`, or start Luz
again. On macOS the OS puts the colours back by itself when Luz quits, even if
it crashed.

**Opening Luz again doesn't seem to do anything.** It did. The second copy
paused or resumed the running one, then quit. Check the sun.

## Build

```powershell
winget install Rustlang.Rustup
choco install innosetup -y     # only for the installer
```

```powershell
.\build.ps1                    # tests + release binary + installer, in dist\
.\build.ps1 -Run               # ...then launch it
.\build.ps1 -Choco             # ...plus the Chocolatey package
.\build.ps1 -Icons             # regenerate assets\icon.ico and the PNGs
.\build.ps1 -SkipInstaller     # binary only
```

```bash
./build.sh                     # tests + release binary + dist/Luz.app
./build.sh --run               # ...then launch it in the menu bar
./build.sh --universal --dmg   # what a release ships: arm64 + x86_64, in a .dmg
```

All you need is the Rust toolchain. The schedule, the colours, the sun and the
menu are shared. Only the OS-specific parts are split: GDI vs CoreGraphics
ramps, the Windows time zone name vs `/etc/localtime`, and WinRT toasts vs
`osascript`. Monitor brightness (`src/backlight.rs`) is Windows only for now,
through the Monitor Configuration API.

`cargo test` covers everything that doesn't need a desktop: sunrise and sunset
against NOAA's full calculator, the schedule through a day with and without
Bedtime mode (and in a polar summer), the colour of each temperature, time zone
lookups, config parsing and the icon drawing. Three tests that touch the real
screens (two tint them for a moment, one steps a monitor's brightness down and
back) are left out by default; `cargo test -- --ignored` runs them.

The landing page is [`docs/index.html`](docs/index.html), a single static file
served by GitHub Pages from `/docs`.

## Releasing

1. Bump `version` in `Cargo.toml` and add a section to `CHANGELOG.md`.
2. `git tag vX.Y.Z && git push origin vX.Y.Z`

The `release` workflow builds the DMG on macOS and the installer and portable
exe on Windows, puts them on a GitHub Release with checksums, and updates the
Chocolatey and winget manifests with the new version and checksums:

- **Chocolatey** is pushed automatically when the `CHOCO_API_KEY` secret is set.
- **winget**: submit the `manifest-winget` artifact to
  [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs), or use
  `wingetcreate submit`.
- **Homebrew**: [kidchenko/homebrew-tap](https://github.com/kidchenko/homebrew-tap)
  bumps `Casks/luz.rb` itself, right away when the `HOMEBREW_TAP_TOKEN` secret
  is set and otherwise on its daily run.

If you run the workflow by hand from the Actions tab, it's a dry run. It builds
everything and uploads it as artifacts, but doesn't publish anything.

## Not yet

- **Monitor brightness on macOS.** It needs the private services MonitorControl
  and Lunar use; for now Dimmer and Brighter darken the picture there.
- **Brightness on the schedule.** Dimming at sunset along with the colour.
- **Pause for an app.** Off while a photo editor or a game is in front, as
  f.lux does.
- **A signed, notarized Mac build.** No more Gatekeeper step, and
  notifications would show up as Luz.
- **Code signing on Windows.** No more SmartScreen warning.

## License

MIT. The menu's glyphs are from [Lucide](https://lucide.dev) (ISC); see
[assets/icons/LICENSES.md](assets/icons/LICENSES.md).
