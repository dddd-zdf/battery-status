use std::collections::HashMap;

use anyhow::{Context, Result};
use win32_notif::{
    notification::visual::{text::HintStyle, Text},
    NotificationBuilder, ToastsNotifier,
};
#[cfg(windows)]
use windows::{
    core::{Interface, HSTRING, PROPVARIANT},
    Win32::{
        Foundation::BOOL,
        Storage::EnhancedStorage::PKEY_AppUserModel_ID,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        },
        UI::Shell::{
            FOLDERID_Programs, IShellLinkW, PropertiesSystem::IPropertyStore, SHGetKnownFolderPath,
            SetCurrentProcessExplicitAppUserModelID, ShellLink,
        },
    },
};

use crate::native_devices::PowerState;

#[derive(Clone, Copy, Debug, Default)]
struct AlertState {
    low_alerted: bool,
}

fn should_alert(
    state: &mut AlertState,
    percent: Option<u8>,
    power: PowerState,
    threshold: u8,
) -> bool {
    let threshold = threshold.min(100);
    let eligible = threshold != 0
        && matches!(power, PowerState::Discharging | PowerState::Unknown)
        && percent.is_some_and(|level| level <= threshold);
    // An offline timeout does not create a fresh alert episode on reconnect.
    if threshold == 0
        || matches!(power, PowerState::Charging | PowerState::Full)
        || percent.is_some_and(|level| level > threshold)
    {
        state.low_alerted = false;
        return false;
    }
    if eligible && !state.low_alerted {
        state.low_alerted = true;
        return true;
    }
    false
}

/// Alert history belongs to each physical device, so one battery cannot mute another.
pub struct Notifier {
    toast_notifier: ToastsNotifier,
    alert_state: HashMap<String, AlertState>,
}

impl Notifier {
    pub fn new() -> Result<Self> {
        let app_id = register_notifications_id().context("registering notifications id")?;
        Ok(Self {
            toast_notifier: ToastsNotifier::new(Some(app_id))?,
            alert_state: HashMap::new(),
        })
    }

    pub fn update(
        &mut self,
        stable_id: &str,
        product_name: &str,
        percent: Option<u8>,
        power: PowerState,
        threshold: u8,
    ) {
        let state = self.alert_state.entry(stable_id.to_owned()).or_default();
        if should_alert(state, percent, power, threshold) {
            let level = percent.expect("eligible alerts always have a percentage");
            if let Err(error) =
                self.show_notification(product_name, &format!("Battery low ({level}%)"))
            {
                log::error!("Failed to show notification: {error:?}");
            }
        }
    }

    pub fn show_notification(&mut self, product_name: &str, body: &str) -> Result<()> {
        NotificationBuilder::new()
            .visual(Text::create(0, product_name).with_style(HintStyle::Title))
            .visual(Text::create(1, body).with_style(HintStyle::Body))
            .build(0, &self.toast_notifier, product_name, "battery")
            .context("building notification")?
            .show()
            .context("showing notification")
    }
}

#[cfg(windows)]
pub fn register_notifications_id() -> Result<String> {
    // An unpackaged desktop process needs an All Programs shortcut with this
    // exact AUMID before Windows will show its toast notifications.
    let app_id = "BatteryStatus.App";
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(app_id))
            .context("SetCurrentProcessExplicitAppUserModelID")?;
    }
    install_start_menu_shortcut(app_id)
        .context("registering Start menu shortcut for toast notifications")?;
    Ok(app_id.to_owned())
}

/// Creates or updates the per-user Start-menu shortcut required by the
/// documented Windows desktop-toast contract. It always targets the currently
/// running executable, so a moved portable copy remains registered correctly.
#[cfg(windows)]
fn install_start_menu_shortcut(app_id: &str) -> Result<()> {
    let apartment_initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let result = (|| -> Result<()> {
        let programs =
            unsafe { SHGetKnownFolderPath(&FOLDERID_Programs, Default::default(), None) }
                .context("locating Start menu Programs folder")?;
        let programs_text = unsafe { programs.to_string() };
        unsafe { CoTaskMemFree(Some(programs.0.cast())) };
        let programs = programs_text.context("reading Start menu Programs folder")?;
        let shortcut_path = std::path::Path::new(&programs).join("Battery Status.lnk");
        let executable = std::env::current_exe().context("locating running executable")?;
        let executable = HSTRING::from(executable.to_string_lossy().as_ref());
        let shortcut = HSTRING::from(shortcut_path.to_string_lossy().as_ref());

        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .context("creating Start menu shortcut")?;
            link.SetPath(&executable)
                .context("setting shortcut target")?;
            link.SetDescription(&HSTRING::from("Battery Status"))
                .context("setting shortcut description")?;

            let store: IPropertyStore = link.cast().context("opening shortcut properties")?;
            let app_id = PROPVARIANT::from(app_id);
            store
                .SetValue(&PKEY_AppUserModel_ID, &app_id)
                .context("setting shortcut AppUserModelID")?;
            store.Commit().context("committing shortcut properties")?;

            let persist: IPersistFile = link.cast().context("opening shortcut persistence")?;
            persist
                .Save(&shortcut, BOOL::from(true))
                .context("saving Start menu shortcut")?;
        }
        Ok(())
    })();
    if apartment_initialized {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{should_alert, AlertState};
    use crate::native_devices::PowerState;

    #[test]
    fn zero_disables_and_rearms_an_episode() {
        let mut state = AlertState::default();
        assert!(!should_alert(
            &mut state,
            Some(5),
            PowerState::Discharging,
            0
        ));
        assert!(should_alert(
            &mut state,
            Some(5),
            PowerState::Discharging,
            10
        ));
        assert!(!should_alert(
            &mut state,
            Some(5),
            PowerState::Discharging,
            10
        ));
        assert!(!should_alert(
            &mut state,
            Some(5),
            PowerState::Discharging,
            0
        ));
        assert!(should_alert(
            &mut state,
            Some(5),
            PowerState::Discharging,
            10
        ));
    }

    #[test]
    fn exact_threshold_alerts_only_once() {
        let mut state = AlertState::default();
        assert!(should_alert(
            &mut state,
            Some(10),
            PowerState::Discharging,
            10
        ));
        assert!(!should_alert(
            &mut state,
            Some(10),
            PowerState::Discharging,
            10
        ));
    }

    #[test]
    fn recovery_and_charging_rearm() {
        let mut state = AlertState::default();
        assert!(should_alert(
            &mut state,
            Some(8),
            PowerState::Discharging,
            10
        ));
        assert!(!should_alert(
            &mut state,
            Some(11),
            PowerState::Discharging,
            10
        ));
        assert!(should_alert(
            &mut state,
            Some(8),
            PowerState::Discharging,
            10
        ));
        assert!(!should_alert(&mut state, Some(8), PowerState::Charging, 10));
        assert!(should_alert(
            &mut state,
            Some(8),
            PowerState::Discharging,
            10
        ));
    }

    #[test]
    fn devices_keep_independent_alert_history() {
        let mut headset = AlertState::default();
        let mut mouse = AlertState::default();
        assert!(should_alert(
            &mut headset,
            Some(5),
            PowerState::Discharging,
            10
        ));
        assert!(should_alert(
            &mut mouse,
            Some(5),
            PowerState::Discharging,
            10
        ));
    }
}
