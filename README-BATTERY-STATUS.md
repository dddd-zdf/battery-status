# Battery Status

A small Windows tray indicator for HyperX Cloud III S Wireless and Logitech PRO X SUPERLIGHT 2 DEX. Adapted from aarol's Headset Battery Indicator, with device-specific tray icons and a compact battery panel.

## Run

Open `dist/BatteryStatus.exe`. It runs in the notification area without a main window. Windows may initially put the icons in the tray overflow menu.

There is one icon for the headset and one for the mouse. Hover to identify the device and read its status. Left-click either icon to open the compact battery panel. Set the alert threshold with the minus/plus buttons, which adjust it by 10 percentage points per click, or click the value and type an exact percentage from 0 to 100. 0% turns alerts off; a device alerts once when its battery is at or below the threshold. Preferences are saved automatically. Refresh is in the bottom-right corner. Right-click either tray icon and select Quit to exit. Logs are stored in battery-status.log beside the executable. Readings refresh in the background every 10 minutes, with a 2-second retry interval when the mouse reading is unavailable. Disconnected or unknown readings are not shown as a measured zero.

`dist/BatteryDiagnostics.exe` prints native HID detection and readings in a console. It uses the same device module as the UI. Run it from PowerShell to keep the output visible:

```powershell
./dist/BatteryDiagnostics.exe
```

No vendor application or administrator account is intended to be required for normal HID access. The supported path is the USB wireless receiver (and supported Logitech wired HID connections); Bluetooth is not implemented.

The headset reports a battery percentage but its referenced protocol does not document a charging flag. Its percentage is displayed normally without a charging claim. Alerts are disabled at a threshold of 0%. The app registers a per-user Start menu shortcut for Windows toast delivery. Windows notification settings still control whether banners appear.

## Modules

- `src/native_devices.rs`: device discovery, HID transport, battery protocol handling and plain reading types. No tray or window dependencies in this module.
- `src/lib.rs`: starts the polling worker and passes readings to the UI.
- `src/app.rs`: application controller, tray lifecycle, readings and panel actions.
- `src/panel.rs`: popup layout, drawing and input handling.
- `src/tray_icons.rs`: device glyph and battery percentage images for the tray.
- `src/notify.rs`, `src/settings.rs`: battery alerts and saved preferences.
- `src/bin/diagnostics.rs`: console consumer of the device module.

To add another device, implement its battery query behind the device module and return a reading. Protocol code should never manipulate tray icons; UI code should never open HID handles.

## Build and test

Requires Windows, stable Rust with the MSVC target, and Visual Studio C++ Build Tools plus the Windows SDK. Fetch submodules after cloning:

```powershell
git submodule update --init --recursive
./build.ps1 -Test
```

The script uses the workspace-local Rust installation in `.tools` when present, otherwise the system Cargo. It generates a release app and diagnostics executable in `dist`. No automatic startup entry is created.

## Sources and licensing

Live verification on this PC confirmed that HyperX `03F0:06BE` and Logitech LIGHTSPEED `046D:C54D` are discovered through their native HID protocols. The mouse sends 20-byte HID++ replies on the long HID collection even for 7-byte requests on the short collection; both channels are handled. Battery values are point-in-time readings and can change as the devices are used.

See `THIRD-PARTY-NOTICES.md` for the three upstream projects and pinned revisions. The original upstream README is preserved in `docs/UPSTREAM-README.md`. This modified application is GPL v3; preserve the license and provide corresponding source when redistributing it.
