use anyhow::{Context, Result};
use winreg::enums::HKEY_CURRENT_USER;

#[derive(Debug, Default, Clone, Copy)]
pub struct Settings {
    /// Percentage at or below which a device raises one low-battery alert.
    /// Zero disables battery alerts.
    pub alert_threshold: u8,
}

impl Settings {
    const KEY: &'static str = "Software\\BatteryStatus";

    pub fn load() -> Result<Self> {
        let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(Self::KEY)
            .context("accessing registry key")?;

        let alert_threshold = match key.get_value::<u32, _>("AlertThreshold") {
            Ok(value) => value.min(100) as u8,
            // Preserve the pre-threshold preference for upgrades.
            Err(_) => match key.get_value::<u32, _>("NotificationsEnabled") {
                Ok(value) if value != 0 => 10,
                _ => 0,
            },
        };

        let settings = Settings { alert_threshold };

        log::debug!("Loaded settings: {:?}", settings);
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(Self::KEY)
            .context("accessing registry key")?;

        key.set_value("AlertThreshold", &u32::from(self.alert_threshold.min(100)))
            .context("saving alert threshold to registry")?;
        Ok(())
    }
}
