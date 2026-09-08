//! Presentation-only tray flyout. Hardware polling stays in native_devices.
use crate::native_devices::{DeviceKind, DeviceReading, PowerState};
use anyhow::{Result, anyhow};
use fontdue::{Font, FontSettings};
use softbuffer::{Context, Surface};
use std::{
    num::NonZeroU32,
    rc::Rc,
    time::{Duration, Instant},
};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};
use winit::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{Key, NamedKey},
    platform::windows::WindowAttributesExtWindows,
    window::{CursorIcon, Window, WindowId, WindowLevel},
};

const WIDTH: f64 = 340.0;
const HEIGHT: f64 = 230.0;
const WHITE: [u8; 3] = [238, 240, 242];
const MUTED: [u8; 3] = [148, 153, 161];
const GREEN: [u8; 3] = [104, 216, 154];
const AMBER: [u8; 3] = [244, 188, 92];

// Shared footer geometry keeps drawing, hit testing, and cursor feedback aligned.
const FOOTER_CENTER_Y: f32 = 199.0;
const STEPPER_X: f32 = 174.0;
const STEPPER_WIDTH: f32 = 99.0;
const STEPPER_TOP: f32 = FOOTER_CENTER_Y - 16.0;
const STEPPER_BOTTOM: f32 = FOOTER_CENTER_Y + 17.0;
const STEPPER_MINUS_RIGHT: f32 = STEPPER_X + 30.0;
const STEPPER_VALUE_LEFT: f32 = STEPPER_X + 31.0;
const STEPPER_VALUE_RIGHT: f32 = STEPPER_X + 66.0;
const REFRESH_X: f32 = 289.0;
const REFRESH_WIDTH: f32 = 34.0;
const REFRESH_TOP: f32 = FOOTER_CENTER_Y - 16.0;
const REFRESH_BOTTOM: f32 = FOOTER_CENTER_Y + 16.0;

pub enum PanelAction {
    Refresh,
    SetAlertThreshold(u8),
}

pub struct Panel {
    surface: Surface<Rc<Window>, Rc<Window>>,
    window: Rc<Window>,
    font: Font,
    readings: Vec<DeviceReading>,
    updated: Option<Instant>,
    cursor: (f64, f64),
    alert_threshold: u8,
    threshold_editing: bool,
    threshold_replace_next_digit: bool,
    caret_visible: bool,
    last_caret_toggle: Instant,
    visible: bool,
    shown_at: Option<Instant>,
    outside_buttons_down: bool,
    has_received_focus: bool,
}
impl Panel {
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self> {
        let window = Rc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Battery Status")
                    .with_inner_size(LogicalSize::new(WIDTH, HEIGHT))
                    .with_resizable(false)
                    .with_decorations(false)
                    .with_skip_taskbar(true)
                    .with_window_level(WindowLevel::AlwaysOnTop)
                    .with_visible(false),
            )?,
        );
        let context = Context::new(window.clone()).map_err(|e| anyhow!(e.to_string()))?;
        let surface = Surface::new(&context, window.clone()).map_err(|e| anyhow!(e.to_string()))?;
        let fonts = std::path::PathBuf::from(
            std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()),
        )
        .join("Fonts");
        let font = Font::from_bytes(
            std::fs::read(fonts.join("segoeui.ttf"))?,
            FontSettings::default(),
        )
        .map_err(|e| anyhow!(e))?;
        // Windows 11 supplies the actual rounded native window and shadow.
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window_handle() {
            if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                use windows::Win32::{
                    Foundation::HWND,
                    Graphics::Dwm::{
                        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
                    },
                };
                unsafe {
                    let _ = DwmSetWindowAttribute(
                        HWND(handle.hwnd.get() as *mut _),
                        DWMWA_WINDOW_CORNER_PREFERENCE,
                        &DWMWCP_ROUND as *const _ as *const _,
                        std::mem::size_of_val(&DWMWCP_ROUND) as u32,
                    );
                }
            }
        }
        Ok(Self {
            surface,
            window,
            font,
            readings: Vec::new(),
            updated: None,
            cursor: (-1.0, -1.0),
            alert_threshold: 20,
            threshold_editing: false,
            threshold_replace_next_digit: false,
            caret_visible: false,
            last_caret_toggle: Instant::now(),
            visible: false,
            shown_at: None,
            outside_buttons_down: false,
            has_received_focus: false,
        })
    }
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }
    pub fn set_alert_threshold(&mut self, threshold: u8) {
        self.alert_threshold = threshold.min(100);
        self.window.request_redraw();
    }
    pub fn set_readings(&mut self, readings: &[DeviceReading]) {
        self.readings = readings.to_vec();
        self.updated = Some(Instant::now());
        self.window.request_redraw();
    }
    pub fn hide(&mut self) {
        self.window.set_visible(false);
        self.visible = false;
        self.shown_at = None;
        self.threshold_editing = false;
        self.threshold_replace_next_digit = false;
        self.caret_visible = false;
    }
    pub fn is_visible(&self) -> bool {
        self.visible
    }
    pub fn show_near(&mut self, x: f64, y: f64) {
        let monitor = self
            .window
            .available_monitors()
            .find(|m| {
                let p = m.position();
                let s = m.size();
                x >= p.x as f64
                    && y >= p.y as f64
                    && x < (p.x as f64 + s.width as f64)
                    && y < (p.y as f64 + s.height as f64)
            })
            .or_else(|| self.window.current_monitor());
        let scale = monitor
            .as_ref()
            .map(|m| m.scale_factor())
            .unwrap_or(self.window.scale_factor());
        let (w, h) = (WIDTH * scale, HEIGHT * scale);
        let (mut px, mut py) = (x - w + 20.0 * scale, y - h - 14.0 * scale);
        if let Some(m) = monitor {
            let p = m.position();
            let s = m.size();
            px = px.clamp(
                p.x as f64 + 8.0,
                (p.x as f64 + s.width as f64 - w - 8.0).max(p.x as f64 + 8.0),
            );
            py = py.clamp(
                p.y as f64 + 8.0,
                (p.y as f64 + s.height as f64 - h - 8.0).max(p.y as f64 + 8.0),
            );
        }
        self.window
            .set_outer_position(PhysicalPosition::new(px as i32, py as i32));
        let _ = self
            .window
            .request_inner_size(LogicalSize::new(WIDTH, HEIGHT));
        self.window.set_visible(true);
        self.window.focus_window();
        // winit's focus request can be ignored for a skip-taskbar tool window.
        // Ask Win32 directly as well, so clicking elsewhere dismisses this flyout.
        self.foreground();
        self.visible = true;
        self.shown_at = Some(Instant::now());
        self.outside_buttons_down = true; // tray click which opened us must not dismiss us
        self.has_received_focus = false;
        self.window.request_redraw();
    }
    /// Poll from the application event loop while visible.  This supplements
    /// focus notifications, which Windows does not reliably send to tool windows.
    pub fn tick(&mut self) {
        if !self.visible {
            return;
        }
        if self.threshold_editing && self.last_caret_toggle.elapsed() >= Duration::from_millis(500)
        {
            self.caret_visible = !self.caret_visible;
            self.last_caret_toggle = Instant::now();
            self.window.request_redraw();
        }
        let Some(shown_at) = self.shown_at else {
            return;
        };
        if shown_at.elapsed() < Duration::from_millis(180) {
            return;
        }
        use windows::Win32::{
            Foundation::{POINT, RECT},
            UI::{
                Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
                WindowsAndMessaging::{GetCursorPos, GetWindowRect},
            },
        };
        let Some(hwnd) = self.hwnd() else {
            return;
        };
        unsafe {
            let mut point = POINT::default();
            let mut rect = RECT::default();
            if GetCursorPos(&mut point).is_err() || GetWindowRect(hwnd, &mut rect).is_err() {
                return;
            }
            let buttons_down = (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0
                || (GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000) != 0;
            if !buttons_down {
                self.outside_buttons_down = false;
                return;
            }
            let inside = point.x >= rect.left
                && point.x < rect.right
                && point.y >= rect.top
                && point.y < rect.bottom;
            if !self.outside_buttons_down && !inside {
                self.hide();
            }
            self.outside_buttons_down = true;
        }
    }
    fn hwnd(&self) -> Option<windows::Win32::Foundation::HWND> {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = self.window.window_handle().ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(handle) => {
                Some(windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut _))
            }
            _ => None,
        }
    }
    fn foreground(&self) {
        if let Some(hwnd) = self.hwnd() {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
            }
        }
    }
    pub fn handle_event(&mut self, event: &WindowEvent) -> Option<PanelAction> {
        match event {
            WindowEvent::CloseRequested => self.hide(),
            // A skip-taskbar flyout may never receive focus.  Only trust a focus
            // loss after we have actually observed focus; tick() covers the rest.
            WindowEvent::Focused(true) => self.has_received_focus = true,
            WindowEvent::Focused(false) if self.has_received_focus => self.hide(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => self.hide(),
                    Key::Named(NamedKey::Enter) if self.threshold_editing => {
                        self.threshold_editing = false;
                        self.threshold_replace_next_digit = false;
                        self.caret_visible = false;
                        self.window.request_redraw();
                    }
                    Key::Named(NamedKey::Backspace) if self.threshold_editing => {
                        self.alert_threshold /= 10;
                        self.threshold_replace_next_digit = false;
                        self.caret_visible = true;
                        self.last_caret_toggle = Instant::now();
                        return Some(PanelAction::SetAlertThreshold(self.alert_threshold));
                    }
                    Key::Character(key) if key.eq_ignore_ascii_case("r") => {
                        return Some(PanelAction::Refresh);
                    }
                    Key::Character(key) if self.threshold_editing => {
                        if let Some(digit) = key.chars().next().and_then(|c| c.to_digit(10)) {
                            let value = if self.threshold_replace_next_digit {
                                digit as u8
                            } else {
                                self.alert_threshold
                                    .saturating_mul(10)
                                    .saturating_add(digit as u8)
                                    .min(100)
                            };
                            self.threshold_replace_next_digit = false;
                            self.alert_threshold = value;
                            self.caret_visible = true;
                            self.last_caret_toggle = Instant::now();
                            return Some(PanelAction::SetAlertThreshold(value));
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let s = self.window.scale_factor();
                self.cursor = (position.x / s, position.y / s);
                self.update_cursor();
                self.window.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = (-1.0, -1.0);
                self.window.set_cursor(CursorIcon::Default);
                self.window.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = self.cursor;
                if Self::in_refresh(x, y) {
                    self.finish_threshold_edit();
                    return Some(PanelAction::Refresh);
                } else if Self::in_stepper_minus(x, y) {
                    self.finish_threshold_edit();
                    return Some(PanelAction::SetAlertThreshold(
                        self.alert_threshold.saturating_sub(10),
                    ));
                } else if Self::in_stepper_plus(x, y) {
                    self.finish_threshold_edit();
                    return Some(PanelAction::SetAlertThreshold(
                        self.alert_threshold.saturating_add(10).min(100),
                    ));
                } else if Self::in_stepper_value(x, y) {
                    self.threshold_editing = true;
                    self.threshold_replace_next_digit = true;
                    self.caret_visible = true;
                    self.last_caret_toggle = Instant::now();
                    self.window.request_redraw();
                } else {
                    self.finish_threshold_edit();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.draw() {
                    log::error!("Could not draw battery panel: {error}");
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.window.request_redraw()
            }
            _ => {}
        }
        None
    }
    fn draw(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(());
        };
        let mut pix = Pixmap::new(w.get(), h.get()).ok_or_else(|| anyhow!("Invalid panel size"))?;
        let scale = self.window.scale_factor() as f32;
        pix.fill(Color::from_rgba8(30, 32, 36, 255));
        {
            let mut c = Canvas {
                pix: &mut pix,
                font: &self.font,
                scale,
            };
            if Self::in_refresh(self.cursor.0, self.cursor.1) {
                c.rounded_rect(
                    REFRESH_X,
                    REFRESH_TOP,
                    REFRESH_WIDTH,
                    REFRESH_BOTTOM - REFRESH_TOP,
                    7.0,
                    [48, 51, 57],
                );
            }
            c.refresh_icon(REFRESH_X + REFRESH_WIDTH / 2.0, FOOTER_CENTER_Y, MUTED);
            for (index, kind) in [DeviceKind::Headset, DeviceKind::Mouse].iter().enumerate() {
                let top = 15.0 + index as f32 * 77.0;
                let reading = self.readings.iter().find(|r| r.device_kind == *kind);
                let percentage = reading.and_then(|r| r.battery_percent);
                let power = reading.map_or(PowerState::Offline, |r| r.power);
                let color = if power == PowerState::Charging {
                    GREEN
                } else if percentage.is_some_and(|p| p <= 20) {
                    AMBER
                } else {
                    WHITE
                };
                c.glyph(
                    *kind,
                    16.0,
                    top + 7.0,
                    if percentage.is_some() { color } else { MUTED },
                );
                c.text(
                    if *kind == DeviceKind::Headset {
                        "HyperX Cloud III S"
                    } else {
                        "Logitech PRO X 2 DEX"
                    },
                    71.0,
                    top + 2.0,
                    14.0,
                    WHITE,
                );
                let status = match power {
                    PowerState::Charging => "Charging",
                    PowerState::Full => "Fully charged",
                    PowerState::Discharging => "On battery",
                    PowerState::Unknown => "Connected",
                    PowerState::Offline => "Not connected",
                };
                c.text(
                    status,
                    71.0,
                    top + 24.0,
                    12.0,
                    if power == PowerState::Charging {
                        GREEN
                    } else {
                        MUTED
                    },
                );
                let value = percentage.map_or_else(|| "—".to_string(), |p| format!("{p}%"));
                c.text_right(&value, 317.0, top - 1.0, 24.0, color);
                c.rect(71.0, top + 52.0, 246.0, 4.0, [62, 66, 72]);
                if let Some(p) = percentage {
                    c.rect(
                        71.0,
                        top + 52.0,
                        246.0 * p.min(100) as f32 / 100.0,
                        4.0,
                        color,
                    );
                }
            }
            c.rect(20.0, 170.0, 300.0, 1.0, [57, 60, 66]);
            let title_height = c.text_height("Alert threshold", 13.0);
            let detail_height = c.text_height("0% turns off alerts", 11.0);
            let line_gap = 3.0;
            let group_height = title_height + line_gap + detail_height;
            let title_center = FOOTER_CENTER_Y - group_height / 2.0 + title_height / 2.0;
            let detail_center = title_center + title_height / 2.0 + line_gap + detail_height / 2.0;
            c.text(
                "Alert threshold",
                22.0,
                c.text_y_centered("Alert threshold", title_center, 13.0),
                13.0,
                WHITE,
            );
            c.text(
                "0% turns off alerts",
                22.0,
                c.text_y_centered("0% turns off alerts", detail_center, 11.0),
                11.0,
                MUTED,
            );
            c.stepper(
                STEPPER_X,
                FOOTER_CENTER_Y,
                self.alert_threshold,
                self.threshold_editing,
                self.caret_visible,
                Self::in_stepper_minus(self.cursor.0, self.cursor.1),
                Self::in_stepper_plus(self.cursor.0, self.cursor.1),
            );
        }
        self.surface
            .resize(w, h)
            .map_err(|e| anyhow!(e.to_string()))?;
        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow!(e.to_string()))?;
        for (out, pixel) in buffer.iter_mut().zip(pix.data().chunks_exact(4)) {
            *out = ((pixel[0] as u32) << 16) | ((pixel[1] as u32) << 8) | pixel[2] as u32;
        }
        buffer.present().map_err(|e| anyhow!(e.to_string()))
    }
    fn finish_threshold_edit(&mut self) {
        if self.threshold_editing {
            self.threshold_editing = false;
            self.threshold_replace_next_digit = false;
            self.caret_visible = false;
            self.window.request_redraw();
        }
    }
    fn update_cursor(&self) {
        let (x, y) = self.cursor;
        let icon = if Self::in_refresh(x, y)
            || Self::in_stepper_minus(x, y)
            || Self::in_stepper_plus(x, y)
        {
            CursorIcon::Pointer
        } else if Self::in_stepper_value(x, y) {
            CursorIcon::Text
        } else {
            CursorIcon::Default
        };
        self.window.set_cursor(icon);
    }
    fn in_footer_bounds(x: f64, y: f64, left: f32, right: f32) -> bool {
        (STEPPER_TOP as f64..STEPPER_BOTTOM as f64).contains(&y)
            && (left as f64..right as f64).contains(&x)
    }
    fn in_stepper_minus(x: f64, y: f64) -> bool {
        Self::in_footer_bounds(x, y, STEPPER_X, STEPPER_MINUS_RIGHT)
    }
    fn in_stepper_value(x: f64, y: f64) -> bool {
        Self::in_footer_bounds(x, y, STEPPER_VALUE_LEFT, STEPPER_VALUE_RIGHT)
    }
    fn in_stepper_plus(x: f64, y: f64) -> bool {
        Self::in_footer_bounds(x, y, STEPPER_VALUE_RIGHT, STEPPER_X + STEPPER_WIDTH)
    }
    fn in_refresh(x: f64, y: f64) -> bool {
        (REFRESH_TOP as f64..REFRESH_BOTTOM as f64).contains(&y)
            && (REFRESH_X as f64..(REFRESH_X + REFRESH_WIDTH) as f64).contains(&x)
    }
}
struct Canvas<'a> {
    pix: &'a mut Pixmap,
    font: &'a Font,
    scale: f32,
}
impl Canvas<'_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [u8; 3]) {
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            let mut p = Paint::default();
            p.set_color_rgba8(color[0], color[1], color[2], 255);
            self.pix.fill_rect(
                rect,
                &p,
                Transform::from_scale(self.scale, self.scale),
                None,
            );
        }
    }
    fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: [u8; 3]) {
        let k = 0.552_284_8 * r;
        let mut path = PathBuilder::new();
        path.move_to(x + r, y);
        path.line_to(x + w - r, y);
        path.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
        path.line_to(x + w, y + h - r);
        path.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
        path.line_to(x + r, y + h);
        path.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
        path.line_to(x, y + r);
        path.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
        path.close();
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], 255);
        self.pix.fill_path(
            &path.finish().unwrap(),
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::from_scale(self.scale, self.scale),
            None,
        );
    }
    fn text_right(&mut self, text: &str, right: f32, y: f32, size: f32, color: [u8; 3]) {
        let width = self.text_width(text, size);
        self.text(text, right - width, y, size, color);
    }
    fn text_width(&self, text: &str, size: f32) -> f32 {
        text.chars()
            .map(|ch| self.font.metrics(ch, size * self.scale).advance_width / self.scale)
            .sum()
    }
    /// Returns the `y` argument for text() that places the rendered glyph bounds
    /// around the requested vertical center, rather than relying on font size.
    fn text_y_centered(&self, text: &str, center: f32, size: f32) -> f32 {
        let px = size * self.scale;
        let ascent = self
            .font
            .horizontal_line_metrics(px)
            .map_or(px, |line| line.ascent)
            / self.scale;
        let mut top = f32::INFINITY;
        let mut bottom = f32::NEG_INFINITY;
        for ch in text.chars() {
            let metrics = self.font.metrics(ch, px);
            let glyph_top =
                ascent - metrics.ymin as f32 / self.scale - metrics.height as f32 / self.scale;
            top = top.min(glyph_top);
            bottom = bottom.max(glyph_top + metrics.height as f32 / self.scale);
        }
        if top.is_finite() {
            center - (top + bottom) / 2.0
        } else {
            center
        }
    }
    fn text_height(&self, text: &str, size: f32) -> f32 {
        let px = size * self.scale;
        text.chars()
            .map(|ch| self.font.metrics(ch, px).height as f32 / self.scale)
            .fold(0.0, f32::max)
    }
    fn text(&mut self, text: &str, x: f32, y: f32, size: f32, color: [u8; 3]) {
        let px = size * self.scale;
        let mut cursor = x * self.scale;
        let baseline = y * self.scale
            + self
                .font
                .horizontal_line_metrics(px)
                .map_or(px, |l| l.ascent);
        let width = self.pix.width() as i32;
        let height = self.pix.height() as i32;
        for ch in text.chars() {
            let (m, bitmap) = self.font.rasterize(ch, px);
            let gx = cursor.round() as i32 + m.xmin;
            let gy = baseline.round() as i32 - m.ymin - m.height as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let (xx, yy) = (gx + col as i32, gy + row as i32);
                    if xx < 0 || yy < 0 || xx >= width || yy >= height {
                        continue;
                    }
                    let a = bitmap[row * m.width + col] as u32;
                    let i = ((yy * width + xx) * 4) as usize;
                    let bytes = self.pix.data_mut();
                    for channel in 0..3 {
                        bytes[i + channel] = ((color[channel] as u32 * a
                            + bytes[i + channel] as u32 * (255 - a))
                            / 255) as u8;
                    }
                }
            }
            cursor += m.advance_width;
        }
    }
    fn glyph(&mut self, kind: DeviceKind, x: f32, y: f32, color: [u8; 3]) {
        let mut path = PathBuilder::new();
        match kind {
            DeviceKind::Headset => {
                path.move_to(x + 3.0, y + 28.0);
                path.line_to(x + 3.0, y + 18.0);
                path.cubic_to(x + 3.0, y - 1.0, x + 39.0, y - 1.0, x + 39.0, y + 18.0);
                path.line_to(x + 39.0, y + 29.0);
                path.move_to(x + 4.0, y + 23.0);
                path.line_to(x + 10.0, y + 23.0);
                path.line_to(x + 10.0, y + 37.0);
                path.line_to(x + 4.0, y + 37.0);
                path.close();
                path.move_to(x + 32.0, y + 23.0);
                path.line_to(x + 38.0, y + 23.0);
                path.line_to(x + 38.0, y + 37.0);
                path.line_to(x + 32.0, y + 37.0);
                path.close();
            }
            DeviceKind::Mouse => {
                path.move_to(x + 7.0, y + 18.0);
                path.cubic_to(x + 7.0, y - 4.0, x + 35.0, y - 4.0, x + 35.0, y + 18.0);
                path.line_to(x + 35.0, y + 30.0);
                path.cubic_to(x + 35.0, y + 48.0, x + 7.0, y + 48.0, x + 7.0, y + 30.0);
                path.close();
                path.move_to(x + 21.0, y + 8.0);
                path.line_to(x + 21.0, y + 16.0);
            }
        }
        let mut p = Paint::default();
        p.set_color_rgba8(color[0], color[1], color[2], 255);
        self.pix.stroke_path(
            &path.finish().unwrap(),
            &p,
            &Stroke {
                width: 2.6,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..Default::default()
            },
            Transform::from_scale(self.scale, self.scale),
            None,
        );
    }
    fn stroke(&mut self, path: PathBuilder, color: [u8; 3], width: f32) {
        let mut p = Paint::default();
        p.set_color_rgba8(color[0], color[1], color[2], 255);
        self.pix.stroke_path(
            &path.finish().unwrap(),
            &p,
            &Stroke {
                width,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..Default::default()
            },
            Transform::from_scale(self.scale, self.scale),
            None,
        );
    }
    fn refresh_icon(&mut self, x: f32, y: f32, color: [u8; 3]) {
        let r = 8.0;
        let k = 4.42;
        let mut p = PathBuilder::new();
        // A nearly complete clockwise circle with an unmistakable arrowhead.
        p.move_to(x + 5.7, y + 5.7);
        p.cubic_to(x + k, y + r, x + 2.0, y + r, x, y + r);
        p.cubic_to(x - k, y + r, x - r, y + k, x - r, y);
        p.cubic_to(x - r, y - k, x - k, y - r, x, y - r);
        p.cubic_to(x + k, y - r, x + r, y - k, x + r, y - 2.0);
        p.move_to(x + r - 5.0, y - 2.0);
        p.line_to(x + r, y - 2.0);
        p.line_to(x + r, y - 7.0);
        self.stroke(p, color, 2.0);
    }
    fn stepper(
        &mut self,
        x: f32,
        y: f32,
        value: u8,
        editing: bool,
        caret_visible: bool,
        hover_minus: bool,
        hover_plus: bool,
    ) {
        self.rounded_rect(x, y - 14.0, STEPPER_WIDTH, 28.0, 6.0, [48, 51, 57]);
        if hover_minus {
            self.rounded_rect(x + 2.0, y - 12.0, 27.0, 24.0, 4.0, [62, 66, 73]);
        }
        if hover_plus {
            self.rounded_rect(x + 67.0, y - 12.0, 30.0, 24.0, 4.0, [62, 66, 73]);
        }
        if editing {
            self.rounded_rect(x + 31.0, y - 11.0, 35.0, 22.0, 4.0, [64, 68, 75]);
        }
        let mut minus = PathBuilder::new();
        minus.move_to(x + 10.0, y);
        minus.line_to(x + 20.0, y);
        self.stroke(minus, if hover_minus { WHITE } else { MUTED }, 1.8);
        let mut plus = PathBuilder::new();
        plus.move_to(x + 77.0, y);
        plus.line_to(x + 87.0, y);
        plus.move_to(x + 82.0, y - 5.0);
        plus.line_to(x + 82.0, y + 5.0);
        self.stroke(plus, if hover_plus { WHITE } else { MUTED }, 1.8);
        let number = value.to_string();
        let number_width = self.text_width(&number, 12.0);
        let percent_width = self.text_width("%", 12.0);
        let suffix_gap = 2.0;
        let total_width = number_width + suffix_gap + percent_width;
        let number_x = x + 48.5 - total_width / 2.0;
        let text_y = self.text_y_centered(&number, y, 12.0);
        self.text(&number, number_x, text_y, 12.0, WHITE);
        let percent_x = number_x + number_width + suffix_gap;
        self.text(
            "%",
            percent_x,
            self.text_y_centered("%", y, 12.0),
            12.0,
            MUTED,
        );
        if editing && caret_visible {
            let caret_x = (number_x + number_width + 0.75).min(x + 64.0);
            self.rect(caret_x, y - 8.0, 1.0, 16.0, WHITE);
        }
    }
}
