use anyhow::Context;
use winit::window::Theme;

use crate::native_devices::{DeviceKind, PowerState};

/// A device silhouette above a percentage, drawn on a pixel-aligned 16px grid.
/// The doubled image stays crisp at the standard Windows tray size and on HiDPI.
pub fn generate_device_icon(
    kind: DeviceKind,
    theme: Theme,
    percent: Option<u8>,
    power: PowerState,
) -> anyhow::Result<tray_icon::Icon> {
    tray_icon::Icon::from_rgba(device_icon_rgba(kind, theme, percent, power, 32), 32, 32)
        .context("creating device battery tray icon")
}

/// Return the same tray artwork at a requested size for previews and image QA.
pub fn device_icon_rgba(
    kind: DeviceKind,
    theme: Theme,
    percent: Option<u8>,
    power: PowerState,
    size: u32,
) -> Vec<u8> {
    let unavailable = percent.is_none() || power == PowerState::Offline;
    let color = if unavailable {
        [145, 150, 160, 255]
    } else if kind == DeviceKind::Mouse && power == PowerState::Charging {
        [54, 211, 130, 255]
    } else if percent.is_some_and(|value| value <= 20) {
        [245, 174, 57, 255]
    } else if theme == Theme::Dark {
        [245, 247, 250, 255]
    } else {
        [35, 40, 47, 255]
    };
    let mut native = vec![0; 16 * 16 * 4];
    let (glyph, width, left): (&[u16], u32, u32) = match kind {
        DeviceKind::Headset => (
            &[
                0b0001111000,
                0b0010000100,
                0b0100000010,
                0b1100000011,
                0b1100000011,
                0b1100000011,
                0b0100000010,
            ],
            10,
            3,
        ),
        DeviceKind::Mouse => (
            &[
                0b00111100, 0b01011010, 0b10011001, 0b10000001, 0b10000001, 0b01000010, 0b00111100,
            ],
            8,
            4,
        ),
    };
    for (y, row) in glyph.iter().enumerate() {
        for x in 0..width {
            if row & (1 << (width - x - 1)) != 0 {
                put_pixel(&mut native, 16, left + x, y as u32, color);
            }
        }
    }
    if unavailable {
        for x in [3, 4, 5, 6, 9, 10, 11, 12] {
            put_pixel(&mut native, 16, x, 11, color);
            put_pixel(&mut native, 16, x, 12, color);
        }
    } else {
        let value = percent.unwrap_or_default().min(100);
        if value == 100 {
            // Narrow digit forms preserve the exact full-charge reading.
            let digits = [
                [0b010, 0b110, 0b010, 0b010, 0b010, 0b010, 0b111],
                [0b111, 0b101, 0b101, 0b101, 0b101, 0b101, 0b111],
            ];
            for (index, digit) in [0, 1, 1].into_iter().enumerate() {
                draw_compact_digit(&mut native, &digits[digit], 3, 2 + index as u32 * 4, color);
            }
        } else if value < 10 {
            draw_compact_digit(&mut native, &SMALL_DIGITS[value as usize], 5, 5, color);
        } else {
            draw_compact_digit(
                &mut native,
                &SMALL_DIGITS[(value / 10) as usize],
                5,
                2,
                color,
            );
            draw_compact_digit(
                &mut native,
                &SMALL_DIGITS[(value % 10) as usize],
                5,
                8,
                color,
            );
        }
    }
    let mut output = vec![0; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let source = (((y * 16 / size) * 16 + x * 16 / size) * 4) as usize;
            let target = ((y * size + x) * 4) as usize;
            output[target..target + 4].copy_from_slice(&native[source..source + 4]);
        }
    }
    output
}

fn draw_compact_digit(img: &mut [u8], rows: &[u8; 7], width: u32, left: u32, color: [u8; 4]) {
    for (y, row) in rows.iter().enumerate() {
        for x in 0..width {
            if row & (1 << (width - x - 1)) != 0 {
                put_pixel(img, 16, left + x, 9 + y as u32, color);
            }
        }
    }
}

const SMALL_DIGITS: [[u8; 7]; 10] = [
    [14, 17, 19, 21, 25, 17, 14],
    [4, 12, 4, 4, 4, 4, 14],
    [14, 17, 1, 2, 4, 8, 31],
    [30, 1, 1, 14, 1, 1, 30],
    [2, 6, 10, 18, 31, 2, 2],
    [31, 16, 16, 30, 1, 1, 30],
    [14, 16, 16, 30, 17, 17, 14],
    [31, 1, 2, 4, 8, 8, 8],
    [14, 17, 17, 14, 17, 17, 14],
    [14, 17, 17, 15, 1, 1, 14],
];

pub fn generate_number_icon(
    theme: Theme,
    battery_percent: Option<u8>,
    state: PowerState,
) -> anyhow::Result<tray_icon::Icon> {
    let size = 16;
    let mut img = vec![0u8; (size * size * 4) as usize];

    let color = match state {
        PowerState::Charging => [0, 255, 0, 255], // Green when charging
        PowerState::Discharging if battery_percent.is_some_and(|level| level <= 20) => {
            [255, 0, 0, 255]
        } // Red when low
        // Otherwise, use white for dark theme and black for light theme
        _ if theme == Theme::Dark => [255, 255, 255, 255],
        _ => [0, 0, 0, 255], // Black for light theme
    };

    if battery_percent.is_none() || state == PowerState::Offline {
        // Draw two dashes for unavailable state
        // First dash
        for x in 2..6 {
            for y in 6..8 {
                put_pixel(&mut img, size, x, y, color);
            }
        }
        // Second dash
        for x in 10..14 {
            for y in 6..8 {
                put_pixel(&mut img, size, x, y, color);
            }
        }
    } else {
        // Draw the digits
        let percent = battery_percent.unwrap_or_default() as usize;
        if percent < 10 {
            // Single digit - draw centered
            draw_digit(&mut img, size, percent, 5, 2, color);
        } else if percent == 100 {
            // The original number icon capped at 99.  Slight overlap keeps 100
            // legible in the 16px tray canvas without lying about a full charge.
            draw_digit(&mut img, size, 1, 0, 2, color);
            draw_digit(&mut img, size, 0, 5, 2, color);
            draw_digit(&mut img, size, 0, 10, 2, color);
        } else {
            // Two digits - use full width
            let tens = (percent / 10) as usize;
            let ones = (percent % 10) as usize;
            draw_digit(&mut img, size, tens, 0, 2, color);
            draw_digit(&mut img, size, ones, 8, 2, color);
        }
    }

    tray_icon::Icon::from_rgba(img, size, size).context("creating icon from generated image buffer")
}

pub fn load_from_resource(
    theme: Theme,
    battery_percent: Option<u8>,
    state: PowerState,
) -> anyhow::Result<tray_icon::Icon> {
    let res_id = battery_res_id_for(theme, battery_percent, state);

    tray_icon::Icon::from_resource(res_id, None)
        .with_context(|| format!("loading icon from resource {res_id}"))
}

fn put_pixel(img: &mut [u8], size: u32, x: u32, y: u32, color: [u8; 4]) {
    let idx = ((y * size + x) * 4) as usize;
    if idx + 3 < img.len() {
        img[idx] = color[0];
        img[idx + 1] = color[1];
        img[idx + 2] = color[2];
        img[idx + 3] = color[3];
    }
}

fn draw_digit(img: &mut [u8], size: u32, digit: usize, x: u32, y: u32, color: [u8; 4]) {
    let bitmap = DIGIT_BITMAPS[digit];

    for (row, &row_data) in bitmap.iter().enumerate() {
        for col in 0..6 {
            if (row_data >> (5 - col)) & 1 == 1 {
                put_pixel(img, size, x + col, y + row as u32, color);
            }
        }
    }
}

fn battery_res_id_for(theme: Theme, battery_percent: Option<u8>, state: PowerState) -> u16 {
    let level = match battery_percent {
        None => 1,
        Some(battery_percent) => match battery_percent {
            0..=12 => 1,  // 0%
            13..=37 => 2, // 25%
            38..=62 => 3, // 50%
            63..=87 => 4, // 75%
            _ => 5,       // 100%
        },
    };

    // light mode icons are (10,20,...,50)
    // dark mode icons are (15,25,...,55)
    let theme_offset: u16 = if theme == Theme::Light { 5 } else { 0 };
    // Charging icons are at icon id + 1
    let charging_offset = (state == PowerState::Charging) as u16;

    if battery_percent.is_none() || state == PowerState::Offline {
        10 + theme_offset
    } else {
        level * 10 + theme_offset + charging_offset
    }
}

// Hardcoded digit bitmaps (6x12 pixels each)
// Each digit is represented as 12 rows, each row is a u8 where the lower 6 bits represent pixels
// 1 = pixel on, 0 = pixel off
const DIGIT_BITMAPS: [[u8; 12]; 10] = [
    // 0
    [
        0b111111, 0b110011, 0b110011, 0b110011, 0b110011, 0b110011, 0b110011, 0b110011, 0b110011,
        0b110011, 0b111111, 0b000000,
    ],
    // 1
    [
        0b001100, 0b001100, 0b001100, 0b001100, 0b001100, 0b001100, 0b001100, 0b001100, 0b001100,
        0b001100, 0b001100, 0b000000,
    ],
    // 2
    [
        0b111111, 0b000011, 0b000011, 0b000011, 0b000011, 0b111111, 0b110000, 0b110000, 0b110000,
        0b110000, 0b111111, 0b000000,
    ],
    // 3
    [
        0b111111, 0b000011, 0b000011, 0b000011, 0b000011, 0b111111, 0b000011, 0b000011, 0b000011,
        0b000011, 0b111111, 0b000000,
    ],
    // 4
    [
        0b110011, 0b110011, 0b110011, 0b110011, 0b110011, 0b111111, 0b000011, 0b000011, 0b000011,
        0b000011, 0b000011, 0b000000,
    ],
    // 5
    [
        0b111111, 0b110000, 0b110000, 0b110000, 0b110000, 0b111111, 0b000011, 0b000011, 0b000011,
        0b000011, 0b111111, 0b000000,
    ],
    // 6
    [
        0b111111, 0b110000, 0b110000, 0b110000, 0b110000, 0b111111, 0b110011, 0b110011, 0b110011,
        0b110011, 0b111111, 0b000000,
    ],
    // 7
    [
        0b111111, 0b000011, 0b000011, 0b000011, 0b000110, 0b001100, 0b011000, 0b011000, 0b011000,
        0b011000, 0b011000, 0b000000,
    ],
    // 8
    [
        0b111111, 0b110011, 0b110011, 0b110011, 0b110011, 0b111111, 0b110011, 0b110011, 0b110011,
        0b110011, 0b111111, 0b000000,
    ],
    // 9
    [
        0b111111, 0b110011, 0b110011, 0b110011, 0b110011, 0b111111, 0b000011, 0b000011, 0b000011,
        0b000011, 0b111111, 0b000000,
    ],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_all_icons() {
        for i in 0..=100 {
            load_from_resource(Theme::Dark, Some(i), PowerState::Discharging)
                .expect("dark battery resource");
        }
        for i in 0..=100 {
            load_from_resource(Theme::Light, Some(i), PowerState::Discharging)
                .expect("light battery resource");
        }
    }

    #[test]
    fn known_level_with_unknown_charge_state_keeps_battery_fill() {
        assert_eq!(
            battery_res_id_for(Theme::Dark, Some(62), PowerState::Unknown),
            30
        );
        assert_eq!(
            battery_res_id_for(Theme::Dark, None, PowerState::Unknown),
            10
        );
        assert_eq!(
            battery_res_id_for(Theme::Dark, Some(0), PowerState::Discharging),
            10
        );
        assert_eq!(
            battery_res_id_for(Theme::Dark, Some(100), PowerState::Charging),
            51
        );
    }
}
