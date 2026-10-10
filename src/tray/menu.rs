use ksni::{menu::StandardItem, Icon, MenuItem, OfflineReason, ToolTip, Tray};
use log::{info, warn};
use std::sync::{mpsc::Sender, Arc};

use crate::config;
use crate::device::{Control, MouseStatus, PollGate};

pub struct BatteryTray {
    battery: Option<MouseStatus>,
    control: Sender<Control>,
    gate: Arc<PollGate>,
    layout_flip: bool,
}

impl BatteryTray {
    pub fn new(control: Sender<Control>, gate: Arc<PollGate>) -> Self {
        Self {
            battery: None,
            control,
            gate,
            layout_flip: false,
        }
    }

    pub fn set_battery(&mut self, status: MouseStatus) -> u8 {
        let previous = self.battery.map_or(100, |s| s.battery_level);
        if self.battery != Some(status) {
            self.layout_flip = !self.layout_flip;
        }
        self.battery = Some(status);
        previous
    }

    fn battery_label(&self) -> String {
        match self.battery {
            Some(s) => format!("Battery: {}%", s.battery_level),
            None => "Battery: reading...".to_string(),
        }
    }

    /// "Charging" or "Discharging", plus the cell voltage when known.
    fn state_label(s: MouseStatus) -> String {
        let state = if s.is_charging {
            "Charging"
        } else {
            "Discharging"
        };
        match s.voltage_mv {
            Some(mv) => format!("{state} \u{b7} {}", volts(mv)),
            None => state.into(),
        }
    }

    fn wake_worker(&self, msg: Control) {
        let _ = self.control.send(msg);
    }

    fn open_configurator(&self) {
        self.gate.pause_for(config::poll_pause_duration());
        self.wake_worker(Control::GateChanged);
        crate::browser::open_configurator();
    }
}

fn volts(mv: u16) -> String {
    format!("{:.2} V", f32::from(mv) / 1000.0)
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
                Some(s) => Self::state_label(s),
                None => "Pulsar X2 CrazyLight".into(),
            },
            ..Default::default()
        }
    }

    fn menu_about_to_show(&mut self) {}

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut battery_text = self.battery_label();
        if let Some(s) = self.battery {
            if let Some(mv) = s.voltage_mv {
                battery_text += &format!(" \u{b7} {}", volts(mv));
            }
            if s.is_charging {
                battery_text += " ⚡";
            }
        }

        let paused = self.gate.is_paused();

        let mut items = vec![
            StandardItem {
                label: battery_text,
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Open configurator".into(),
                icon_name: "applications-internet".into(),
                activate: Box::new(|this: &mut Self| this.open_configurator()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Refresh now".into(),
                icon_name: "view-refresh-symbolic".into(),
                activate: Box::new(|this: &mut Self| {
                    this.gate.resume();
                    this.wake_worker(Control::Refresh);
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
                activate: Box::new(|this: &mut Self| {
                    this.gate.toggle();
                    this.wake_worker(Control::GateChanged);
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
        ];

        if self.layout_flip {
            items.push(
                StandardItem {
                    visible: false,
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }
        items
    }

    fn watcher_online(&self) {
        info!("Tray watcher online");
    }

    fn watcher_offline(&self, _reason: OfflineReason) -> bool {
        warn!("Tray watcher offline; keeping service alive, ksni will re-register");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tray() -> BatteryTray {
        BatteryTray::new(std::sync::mpsc::channel().0, Arc::new(PollGate::new()))
    }

    fn status(level: u8) -> MouseStatus {
        MouseStatus {
            battery_level: level,
            is_charging: false,
            voltage_mv: Some(4100),
        }
    }

    #[test]
    fn status_change_changes_the_layout() {
        let mut t = tray();
        t.set_battery(status(100));
        let before = t.menu().len();
        t.set_battery(status(95));
        assert_ne!(t.menu().len(), before);
    }

    #[test]
    fn same_status_keeps_the_layout() {
        let mut t = tray();
        t.set_battery(status(100));
        let before = t.menu().len();
        t.set_battery(status(100));
        assert_eq!(t.menu().len(), before);
    }
}
