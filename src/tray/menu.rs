use ksni::{menu::StandardItem, Icon, MenuItem, OfflineReason, ToolTip, Tray};
use log::{info, warn};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use crate::config;
use crate::device::PollGate;
use crate::tray::notifications::NotificationState;

#[derive(Debug, Clone)]
pub struct BatteryContext {
    pub battery: Option<(u8, bool)>, // (level, is_charging)
    pub notifications: NotificationState,
}

impl Default for BatteryContext {
    fn default() -> Self {
        Self {
            battery: None,
            notifications: NotificationState::new(),
        }
    }
}

pub struct BatteryTray {
    pub ctx: Arc<Mutex<BatteryContext>>,
    pub refresh_flag: Arc<AtomicBool>,
    pub gate: Arc<PollGate>,
}

impl BatteryTray {
    fn open_configurator(&self) {
        self.gate.pause_for(config::poll_pause_duration());
        crate::browser::open_configurator();
    }
}

impl Tray for BatteryTray {
    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let ctx = self.ctx.lock().unwrap();
        match ctx.battery {
            Some((level, charging)) => crate::tray::icon::get_pixmaps(level, charging),
            None => crate::tray::icon::get_placeholder_pixmaps(),
        }
    }

    fn id(&self) -> String {
        "litecrazy-battery".into()
    }

    fn title(&self) -> String {
        let ctx = self.ctx.lock().unwrap();
        match ctx.battery {
            Some((level, _)) => format!("Battery: {level}%"),
            None => "Battery: reading...".to_string(),
        }
    }

    fn tool_tip(&self) -> ToolTip {
        let ctx = self.ctx.lock().unwrap();
        let (title, description) = match ctx.battery {
            Some((level, charging)) => (
                format!("Battery: {level}%"),
                if charging { "Charging" } else { "Discharging" }.to_string(),
            ),
            None => (
                "Pulsar X2 CrazyLight".to_string(),
                "Reading battery...".to_string(),
            ),
        };
        ToolTip {
            title,
            description,
            icon_name: String::default(),
            icon_pixmap: Vec::default(),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let ctx = self.ctx.lock().unwrap();
        let battery_text = match ctx.battery {
            Some((level, charging)) => {
                format!("Battery: {}%{}", level, if charging { " ⚡" } else { "" })
            }
            None => "Battery: reading...".to_string(),
        };

        let paused = self.gate.is_paused();
        let pause_label = if paused {
            "Resume battery polling"
        } else {
            "Pause battery polling"
        };

        vec![
            StandardItem {
                label: battery_text,
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Open Configurator".into(),
                icon_name: "applications-internet".into(),
                activate: Box::new(|this: &mut Self| {
                    this.open_configurator();
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Refresh Now".into(),
                icon_name: "view-refresh-symbolic".into(),
                activate: Box::new(|this: &mut Self| {
                    this.gate.resume();
                    this.refresh_flag.store(true, Ordering::Release);
                    info!("Refresh requested via tray menu");
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: pause_label.into(),
                icon_name: if paused {
                    "media-playback-start-symbolic".into()
                } else {
                    "media-playback-pause-symbolic".into()
                },
                activate: Box::new(|this: &mut Self| {
                    this.gate.toggle();
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Exit".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|_| {
                    info!("Exiting tray");
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_online(&self) {
        info!("Tray watcher online");
    }

    fn watcher_offline(&self, _reason: OfflineReason) -> bool {
        warn!("Tray watcher offline; keeping service alive, ksni will re-register");
        true
    }
}
