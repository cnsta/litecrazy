use std::{
    sync::{Arc, mpsc::Receiver},
    time::Duration,
};

use anyhow::Context;
use ksni::blocking::TrayMethods;
use log::{info, warn};

use crate::{
    config,
    device::{BatteryEvent, BatteryWorker, Control, MouseStatus, PollGate, WorkerConfig},
    tray::{menu::BatteryTray, notifications::NotificationState},
};

const DISCONNECT_BACKOFF: Duration = Duration::from_secs(5);

const INITIAL_READING_TIMEOUT: Duration = Duration::from_secs(5);

pub fn run() -> anyhow::Result<()> {
    info!("Starting litecrazy battery tray");

    let gate = Arc::new(PollGate::new());

    let interval = config::battery_interval();
    info!("Battery monitoring: checking every {}s", interval.as_secs());

    let (worker, events) = BatteryWorker::spawn(WorkerConfig {
        interval,
        disconnect_backoff: DISCONNECT_BACKOFF,
        gate: gate.clone(),
    });

    // Stopping the worker closes `events`, which ends the loop below.
    {
        let control = worker.control();
        ctrlc::set_handler(move || {
            info!("Received shutdown signal");
            let _ = control.send(Control::Shutdown);
        })
        .context("Failed to set signal handler")?;
    }

    let mut notifications = NotificationState::new();
    let mut tray = BatteryTray::new(worker.control(), gate);

    match wait_for_first_reading(&events, INITIAL_READING_TIMEOUT) {
        Some(status) => {
            info!(
                "Initial battery: {}%{}",
                status.battery_level,
                if status.is_charging { " ⚡" } else { "" }
            );
            let previous = tray.set_battery(status);
            maybe_notify(&mut notifications, status, previous);
        }
        None => info!("No reading yet; showing a placeholder until the mouse answers"),
    }

    // ksni registers with the StatusNotifierWatcher whenever one appears, so
    // there's no need to wait for it here.
    let handle = tray
        .assume_sni_available(true)
        .spawn()
        .context("Failed to spawn tray icon")?;

    info!("Tray icon spawned successfully");

    for event in &events {
        match event {
            BatteryEvent::Update(status) => {
                handle_battery_update(&handle, &mut notifications, status)
            }
            BatteryEvent::Asleep => log::debug!("Mouse asleep"),
            BatteryEvent::Disconnected => {
                info!("Device unreachable; waiting for it to come back")
            }
        }
    }

    info!("Tray service shutting down");
    drop(worker);
    Ok(())
}

fn wait_for_first_reading(
    events: &Receiver<BatteryEvent>,
    timeout: Duration,
) -> Option<MouseStatus> {
    match events.recv_timeout(timeout) {
        Ok(BatteryEvent::Update(status)) => Some(status),
        _ => None,
    }
}

fn handle_battery_update(
    handle: &ksni::blocking::Handle<BatteryTray>,
    notifications: &mut NotificationState,
    status: MouseStatus,
) {
    info!(
        "Battery: {}%{}",
        status.battery_level,
        if status.is_charging { " ⚡" } else { "" }
    );

    let Some(previous) = handle.update(|tray| tray.set_battery(status)) else {
        warn!("Tray service is gone; battery update dropped");
        return;
    };

    maybe_notify(notifications, status, previous);
}

fn maybe_notify(notifications: &mut NotificationState, status: MouseStatus, previous: u8) {
    let threshold = config::low_battery_threshold();
    if notifications.should_notify_low_battery(
        status.battery_level,
        previous,
        threshold,
        status.is_charging,
    ) && let Err(e) = notifications.send_low_battery(status.battery_level)
    {
        warn!("Failed to send low-battery notification: {e}");
    }
}
