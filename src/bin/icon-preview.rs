//! Render the actual tray artwork at native sizes, with a larger inspection row.
use battery_status::{
    native_devices::{DeviceKind, PowerState},
    tray_icons::device_icon_rgba,
};
use winit::window::Theme;

fn main() -> anyhow::Result<()> {
    let mut preview = tiny_skia::Pixmap::new(1000, 200).unwrap();
    preview.fill(tiny_skia::Color::from_rgba8(30, 32, 36, 255));
    for (column, size) in [16, 20, 24, 32].into_iter().enumerate() {
        for (device, (kind, percent)) in [(DeviceKind::Headset, 62), (DeviceKind::Mouse, 79)]
            .into_iter()
            .enumerate()
        {
            for (row, scale) in [1, 3].into_iter().enumerate() {
                let pixels =
                    device_icon_rgba(kind, Theme::Dark, Some(percent), PowerState::Unknown, size);
                let left = 10 + column as u32 * 250 + device as u32 * 110;
                let top = if row == 0 { 15 } else { 70 };
                for y in 0..size {
                    for x in 0..size {
                        let source = ((y * size + x) * 4) as usize;
                        if pixels[source + 3] == 0 {
                            continue;
                        }
                        for dy in 0..scale {
                            for dx in 0..scale {
                                let px = left + x * scale + dx;
                                let py = top + y * scale + dy;
                                if px < 1000 && py < 200 {
                                    let index = ((py * 1000 + px) * 4) as usize;
                                    preview.data_mut()[index..index + 4]
                                        .copy_from_slice(&pixels[source..source + 4]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    std::fs::create_dir_all("output")?;
    preview.save_png("output/tray-icons.png")?;
    Ok(())
}
