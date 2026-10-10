# LITECRAZY

A battery tray icon for the Pulsar X2 CrazyLight on Linux.

> [!NOTE]
> Device configuration now happens at **<https://bbb.pulsar.gg/>**, which talks
> to the mouse directly from the browser over WebHID. This is unfortunate but I
> ran into far too many bugs and I simply don't have the time nor energy to
> reverse engineer this piece of shit mouse.

## Features

- Battery level, charging status and cell voltage in the system tray
- Low-battery desktop notification (configurable threshold)
- Menu shortcut that opens the configurator in a Chromium-based browser
- Automatically pauses polling while the configurator is open, so the tray and
  the web page aren't driving the same HID node at once

> [!WARNING]
> Why Chromium? The configurator uses WebHID. Firefox has declined to implement
> it, rightly citing security concerns. Use WebHID at your own risk.

The application looks for a Chromium binary in the following order:

1. `$LITECRAZY_BROWSER`, if set
2. The desktop's default browser (`xdg-settings get default-web-browser`), if it
   is one of the Chromium browsers below
3. `chromium`, `chromium-browser`, `google-chrome-stable`, `google-chrome`,
   `brave-browser`, `brave`, `vivaldi-stable`, `vivaldi`,
   `microsoft-edge-stable`, `microsoft-edge`, `thorium-browser`,
   `ungoogled-chromium`, `opera`
4. The same browsers packaged as Flatpaks
5. `xdg-open`, plus a notification explaining why the page may not work

The window is opened with `--app=` and `--class=litecrazy`, so it comes up
chrome-less and can be given a compositor rule. Set `LITECRAZY_WINDOW_MODE=tab`
if you'd rather have a normal tab.

```
# Hyprland
windowrulev2 = float, class:^(litecrazy)$
```

## Installation

### NixOS

```nix
# flake.nix
inputs.litecrazy = {
  url = "github:cnsta/litecrazy";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

```nix
# configuration.nix
hardware.litecrazy = {
  enable = true;               # installs package + udev rules
  service = {
    enable = true;             # systemd user service
    browser = pkgs.chromium;   # optional; auto-detected when unset
    batteryInterval = 60;
    lowBatteryThreshold = 20;
  };
};
```

### Build from source

```bash
cargo build --release
```

Runtime dependencies: `libudev`, `libdbus`. On NixOS these are handled by the
package derivation.

## Usage

```bash
litecrazy          # start the tray (default)
litecrazy --open   # open the configurator and exit — handy for a hotkey
litecrazy --dump   # print the raw online/battery replies (for bug reports)
litecrazy --help
```

## Configuration

There is no config file. The handful of remaining knobs are environment
variables, which the NixOS module sets for you.

| Variable                  | Default                  | Meaning                                                   |
| ------------------------- | ------------------------ | --------------------------------------------------------- |
| `LITECRAZY_URL`           | `https://bbb.pulsar.gg/` | Configurator URL                                          |
| `LITECRAZY_BROWSER`       | _(auto-detect)_          | Browser binary, name or absolute path                     |
| `LITECRAZY_BROWSER_ARGS`  | _(none)_                 | Extra flags for the browser                               |
| `LITECRAZY_WINDOW_MODE`   | `app`                    | `app` for a chrome-less window, `tab` for an ordinary tab |
| `LITECRAZY_INTERVAL`      | `60`                     | Battery poll interval, seconds (10–3600)                  |
| `LITECRAZY_LOW_THRESHOLD` | `20`                     | Notification threshold, percent (0 = off)                 |
| `LITECRAZY_PAUSE_MINUTES` | `10`                     | Polling pause after opening the configurator (0 = off)    |

Polling backs off automatically while the mouse is asleep, up to 8× the base
interval, so a low value is less costly than it looks.

## USB permissions

The same udev rules that let litecrazy read the battery are what let the browser
reach the mouse from the configurator page. On non-NixOS systems, create
`/etc/udev/rules.d/99-litecrazy.rules`:

```
SUBSYSTEM=="usb",    ATTRS{idVendor}=="3710", ATTRS{idProduct}=="3414", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="usb",    ATTRS{idVendor}=="3710", ATTRS{idProduct}=="5406", MODE="0666", TAG+="uaccess"
KERNEL=="hidraw*",   ATTRS{idVendor}=="3710", ATTRS{idProduct}=="3414", MODE="0666", TAG+="uaccess"
KERNEL=="hidraw*",   ATTRS{idVendor}=="3710", ATTRS{idProduct}=="5406", MODE="0666", TAG+="uaccess"
```

```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
```

## Troubleshooting

Since I exclusively use NixOS and Hyprland, these are the only environments that
have been thoroughly tested. Feel free to open an issue if you encounter bugs.

**Device not found**: check `lsusb | grep 3710`, verify udev rules

**Tray not visible**: requires a StatusNotifier-compatible desktop (KDE, GNOME
with AppIndicator extension, most others). Check
`journalctl --user -u litecrazy`.

**Configurator opens in Firefox**: no Chromium binary was found on `PATH`. Set
`LITECRAZY_BROWSER` to the one you want, or `hardware.litecrazy.service.browser`
on NixOS.

**Configurator can't see the mouse**: check the udev rules above, and that the
page is on HTTPS — WebHID is refused on insecure origins.

**Battery reading goes stale**: polling pauses for ten minutes after you open
the configurator. "Refresh Now" resumes it immediately, or set
`LITECRAZY_PAUSE_MINUTES=0` to turn the behaviour off.

## Credits

- [Elehiggle](https://github.com/Elehiggle/SimplePulsarBatteryNotification) -
  protocol reverse engineering and battery command sequences
- [NotAShelf/tailray](https://github.com/NotAShelf/tailray) - tray architecture
  reference

## Disclaimer

This project is not affiliated with Pulsar Gaming Gears or AplusX Inc.
