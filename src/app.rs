use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::Context;
use log::{error, info, warn};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
};

use crate::panel::{Panel, PanelAction};
use crate::{
    native_devices::{DeviceKind, DeviceReading, PowerState},
    notify::Notifier,
    settings::Settings,
    tray_icons,
};

use tray_icon::menu::{Menu, MenuEvent, MenuItem};

enum AppEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

struct DeviceTray {
    kind: DeviceKind,
    tray: TrayIcon,
    last_text: String,
    quit: MenuItem,
}

pub struct AppState {
    settings: Settings,
    notifier: Option<Notifier>,
    headset: DeviceTray,
    mouse: DeviceTray,
    readings: mpsc::Receiver<Vec<DeviceReading>>,
    refresh: mpsc::Sender<()>,
    latest: Vec<DeviceReading>,
    panel: Option<Panel>,
}

pub fn run(
    readings: mpsc::Receiver<Vec<DeviceReading>>,
    refresh: mpsc::Sender<()>,
) -> anyhow::Result<()> {
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .context("initializing event loop")?;
    let tray_proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = tray_proxy.send_event(AppEvent::Tray(event));
    }));
    let menu_proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = menu_proxy.send_event(AppEvent::Menu(event));
    }));
    let mut app = AppState::new(readings, refresh)?;
    event_loop
        .run_app(&mut app)
        .context("running tray application")
}

impl AppState {
    fn new(
        readings: mpsc::Receiver<Vec<DeviceReading>>,
        refresh: mpsc::Sender<()>,
    ) -> anyhow::Result<Self> {
        let settings = Settings::load().context("loading settings")?;
        let headset = Self::new_device_tray(DeviceKind::Headset, "Headset")?;
        let mouse = Self::new_device_tray(DeviceKind::Mouse, "Mouse")?;
        let notifier = Notifier::new()
            .map_err(|err| {
                warn!("Notifications unavailable: {err:?}");
                err
            })
            .ok();
        Ok(Self {
            settings,
            notifier,
            headset,
            mouse,
            readings,
            refresh,
            latest: Vec::new(),
            panel: None,
        })
    }

    fn new_device_tray(kind: DeviceKind, label: &str) -> anyhow::Result<DeviceTray> {
        let icon = tray_icons::generate_device_icon(
            kind,
            crate::system_theme(),
            None,
            PowerState::Offline,
        )?;
        let menu = Menu::new();
        let quit = MenuItem::new("Quit", true, None);
        menu.append(&quit)?;
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip(format!("{label}: disconnected"))
            .with_menu_on_left_click(false)
            .with_menu(Box::new(menu))
            .with_menu_on_right_click(true)
            .build()
            .context("creating tray icon")?;
        Ok(DeviceTray {
            kind,
            tray,
            last_text: String::new(),
            quit,
        })
    }

    fn apply_readings(&mut self, readings: Vec<DeviceReading>) {
        self.apply_kind(&readings, DeviceKind::Headset);
        self.apply_kind(&readings, DeviceKind::Mouse);
        if let Some(panel) = &mut self.panel {
            panel.set_readings(&readings);
        }
        self.latest = readings;
    }

    fn apply_kind(&mut self, readings: &[DeviceReading], kind: DeviceKind) {
        let reading = readings.iter().find(|reading| reading.device_kind == kind);
        let tray = if kind == DeviceKind::Headset {
            &mut self.headset
        } else {
            &mut self.mouse
        };
        let (name, stable_id, percent, power) = match reading {
            Some(reading) => (
                reading.name.as_str(),
                Some(reading.stable_id.as_str()),
                reading.battery_percent,
                reading.power,
            ),
            None => (
                if kind == DeviceKind::Headset {
                    "Headset"
                } else {
                    "Mouse"
                },
                None,
                None,
                PowerState::Offline,
            ),
        };
        let state_text = device_text(name, percent, power);
        if tray.last_text != state_text {
            info!("Applied {:?} reading: {}", tray.kind, state_text);
            tray.last_text.clone_from(&state_text);
        }
        if let Err(err) = tray.tray.set_tooltip(Some(&state_text)) {
            error!("setting tooltip: {err:?}");
        }
        match Self::load_icon(kind, percent, power) {
            Ok(icon) => {
                if let Err(err) = tray.tray.set_icon(Some(icon)) {
                    error!("setting tray icon: {err:?}");
                }
            }
            Err(err) => error!("building tray icon: {err:?}"),
        }
        if let (Some(notifier), Some(stable_id)) = (&mut self.notifier, stable_id) {
            notifier.update(
                stable_id,
                name,
                percent,
                power,
                self.settings.alert_threshold,
            );
        }
    }

    fn load_icon(
        kind: DeviceKind,
        percent: Option<u8>,
        power: PowerState,
    ) -> anyhow::Result<tray_icon::Icon> {
        tray_icons::generate_device_icon(kind, crate::system_theme(), percent, power)
    }
}

impl ApplicationHandler<AppEvent> for AppState {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.panel.is_none() {
            match Panel::new(event_loop) {
                Ok(mut panel) => {
                    panel.set_readings(&self.latest);
                    panel.set_alert_threshold(self.settings.alert_threshold);
                    if std::env::args().any(|arg| arg == "--show") {
                        if let Some(rect) = self.headset.tray.rect() {
                            panel.show_near(rect.position.x, rect.position.y);
                        } else {
                            panel.show_near(1000.0, 800.0);
                        }
                    }
                    self.panel = Some(panel);
                }
                Err(err) => error!("Unable to create battery detail panel: {err:?}"),
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_secs(1),
        ));
    }
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_secs(1),
            ));
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        while let Ok(readings) = self.readings.try_recv() {
            self.apply_readings(readings);
        }
        if let Some(panel) = &mut self.panel {
            panel.tick();
        }
        let delay = if self.panel.as_ref().is_some_and(|panel| panel.is_visible()) {
            Duration::from_millis(50)
        } else {
            Duration::from_secs(1)
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + delay));
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Menu(event)
                if event.id == self.headset.quit.id() || event.id == self.mouse.quit.id() =>
            {
                event_loop.exit()
            }
            AppEvent::Tray(TrayIconEvent::Click {
                id,
                position,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }) if id == *self.headset.tray.id() || id == *self.mouse.tray.id() => {
                if let Some(panel) = &mut self.panel {
                    panel.show_near(position.x, position.y);
                }
            }
            _ => {}
        }
    }
    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let action = self
            .panel
            .as_mut()
            .filter(|panel| panel.window_id() == id)
            .and_then(|panel| panel.handle_event(&event));
        match action {
            Some(PanelAction::Refresh) => {
                let _ = self.refresh.send(());
            }
            Some(PanelAction::SetAlertThreshold(threshold)) => {
                self.settings.alert_threshold = threshold.min(100);
                if let Some(panel) = &mut self.panel {
                    panel.set_alert_threshold(self.settings.alert_threshold);
                }
                if let Err(err) = self.settings.save() {
                    error!("saving settings: {err:?}");
                }
                self.apply_readings(self.latest.clone());
            }

            None => {}
        }
    }
    fn exiting(&mut self, _: &ActiveEventLoop) {
        info!("Exiting application");
    }
}

fn device_text(name: &str, percent: Option<u8>, power: PowerState) -> String {
    match (percent, power) {
        (Some(level), PowerState::Charging) => format!("{name}: {level}% (Charging)"),
        (Some(level), PowerState::Full) => format!("{name}: {level}% (Full)"),
        (Some(level), _) => format!("{name}: {level}%"),
        (_, PowerState::Offline) => format!("{name}: disconnected"),
        _ => format!("{name}: battery unavailable"),
    }
}
