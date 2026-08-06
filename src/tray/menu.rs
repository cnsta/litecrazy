use ksni::{menu::StandardItem, Icon, MenuItem, OfflineReason, ToolTip, Tray};
use log::{info, warn};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::config;
use crate::device::{MouseStatus, PollGate};

pub struct BatteryTray {
    battery: Option<MouseStatus>,
    refresh_flag: Arc<AtomicBool>,
    gate: Arc<PollGate>,
}

impl BatteryTray {
    pub fn new(refresh_flag: Arc<AtomicBool>, gate: Arc<PollGate>) -> Self {
        Self {
            battery: None,
            refresh_flag,
            gate,
        }
    }

    pub fn set_battery(&mut self, status: MouseStatus) -> u8 {
        let previous = self.battery.map_or(100, |s| s.battery_level);
        self.battery = Some(status);
        previous
    }

    fn battery_label(&self) -> String {
        match self.battery {
            Some(s) => format!("Battery: {}%", s.battery_level),
            None => "Battery: reading...".to_string(),
        }
    }

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
        match self.battery {
            Some(s) => crate::tray::icon::get_pixmaps(s.battery_level, s.is_charging),
            None => crate::tray::icon::get_placeholder_pixmaps(),
        }
    }

    fn id(&self) -> String {
        "litecrazy-battery".into()
    }

    fn title(&self) -> String {
        self.battery_label()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: self.battery_label(),
            description: match self.battery {
                Some(s) if s.is_charging => "Charging".into(),
                Some(_) => "Discharging".into(),
                None => "Pulsar X2 CrazyLight".into(),
            },
            ..Default::default()
        }
    }

    fn menu_about_to_show(&mut self) {}

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let battery_text = match self.battery {
            Some(s) if s.is_charging => format!("{} ⚡", self.battery_label()),
            _ => self.battery_label(),
        };

        let paused = self.gate.is_paused();

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
                activate: Box::new(|this: &mut Self| this.open_configurator()),
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
                label: if paused {
                    "Resume battery polling".into()
                } else {
                    "Pause battery polling".into()
                },
                icon_name: if paused {
                    "media-playback-start-symbolic".into()
                } else {
                    "media-playback-pause-symbolic".into()
                },
                activate: Box::new(|this: &mut Self| this.gate.toggle()),
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
