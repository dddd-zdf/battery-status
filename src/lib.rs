//! BatteryStatus keeps native device discovery separate from tray presentation.

mod app;
pub mod native_devices;
mod notify;
mod panel;
mod settings;
pub mod tray_icons;

use std::{
    sync::{mpsc, LazyLock},
    thread,
    time::Duration,
};
use windows::{
    core::PCSTR,
    Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA},
};
use winit::window::Theme;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Owns background polling. UI receives readings and can request an immediate
/// refresh, but never calls a native HID reader itself.
pub fn run() -> anyhow::Result<()> {
    let (readings_tx, readings_rx) = mpsc::channel();
    let (refresh_tx, refresh_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut had_mouse = false;
        loop {
            let mut readings = native_devices::poll_devices();
            // Confirm a dropped wireless reading before clearing a previously
            // visible percentage; brief receiver wake-ups can reject a query.
            if had_mouse
                && !readings.iter().any(|reading| {
                    reading.device_kind == native_devices::DeviceKind::Mouse
                        && reading.battery_percent.is_some()
                })
            {
                thread::sleep(Duration::from_millis(200));
                readings = native_devices::poll_devices();
            }
            let mouse_available = readings.iter().any(|reading| {
                reading.device_kind == native_devices::DeviceKind::Mouse
                    && reading.battery_percent.is_some()
            });
            had_mouse = mouse_available;
            if readings_tx.send(readings).is_err() {
                break;
            }
            // Popup refresh actions interrupt the ordinary 30-second cadence.
            let interval = if mouse_available { 30 } else { 2 };
            if matches!(
                refresh_rx.recv_timeout(Duration::from_secs(interval)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ) {
                break;
            }
        }
    });
    app::run(readings_rx, refresh_tx)
}

pub(crate) fn system_theme() -> Theme {
    type ShouldSystemUseDarkMode = unsafe extern "system" fn() -> bool;
    static SHOULD_DARK: LazyLock<Option<ShouldSystemUseDarkMode>> = LazyLock::new(|| unsafe {
        let ordinal = PCSTR::from_raw(138u16 as *const u8);
        LoadLibraryA(windows::core::s!("uxtheme.dll"))
            .ok()
            .and_then(|module| GetProcAddress(module, ordinal))
            .map(|proc| std::mem::transmute(proc))
    });
    if SHOULD_DARK
        .map(|function| unsafe { function() })
        .unwrap_or(true)
    {
        Theme::Dark
    } else {
        Theme::Light
    }
}
