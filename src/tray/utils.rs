use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::RecvTimeoutError,
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use ksni::blocking::TrayMethods;
use log::{info, warn};

use crate::{
    config,
    device::{BatteryEvent, BatteryWorker, MouseStatus, PollGate, WorkerConfig},
    tray::menu::{BatteryContext, BatteryTray},
};

/// How long to wait for the device to come back after a failed poll.
const DISCONNECT_BACKOFF: Duration = Duration::from_secs(5);

pub fn run() -> anyhow::Result<()> {
    info!("Starting litecrazy battery tray");

    wait_for_watcher(Duration::from_secs(30));

    let ctx = Arc::new(Mutex::new(BatteryContext::default()));
    let refresh_flag = Arc::new(AtomicBool::new(false));
    let gate = Arc::new(PollGate::new());

    let tray = BatteryTray {
        ctx: ctx.clone(),
        refresh_flag: refresh_flag.clone(),
        gate: gate.clone(),
    };
    let handle = tray
        .assume_sni_available(true)
        .spawn()
        .context("Failed to spawn tray icon")?;

    info!("Tray icon spawned successfully");

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = running.clone();
        ctrlc::set_handler(move || {
            info!("Received shutdown signal");
            running.store(false, Ordering::Release);
        })
        .context("Failed to set signal handler")?;
    }

    let interval = config::battery_interval();
    info!("Battery monitoring: checking every {}s", interval.as_secs());

    let (worker, events) = BatteryWorker::spawn(WorkerConfig {
        interval,
        disconnect_backoff: DISCONNECT_BACKOFF,
        refresh_flag,
        gate,
    });
    // Dropping the worker joins its thread, so it must outlive the loop.
    let _worker_guard = worker;

    while running.load(Ordering::Acquire) {
        // Short timeout so shutdown is responsive even when the worker is
        // idle. Like when the mouse has been asleep for hours.
        match events.recv_timeout(Duration::from_millis(500)) {
            Ok(BatteryEvent::Update(status)) => handle_battery_update(&ctx, &handle, status),
            Ok(BatteryEvent::Asleep) => log::debug!("Mouse asleep"),
            Ok(BatteryEvent::Disconnected) => {
                info!("Device unreachable; waiting for it to come back")
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                warn!("Battery worker channel closed; shutting down");
                break;
            }
        }
    }

    info!("Tray service shutting down");
    Ok(())
}

fn wait_for_watcher(timeout: Duration) {
    use zbus::blocking::Connection;

    let Ok(conn) = Connection::session() else {
        warn!("Could not connect to session D-Bus; proceeding without watcher check");
        return;
    };
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
        warn!("Could not create DBus proxy; proceeding without watcher check");
        return;
    };

    let start = Instant::now();
    loop {
        match dbus.list_names() {
            Ok(names)
                if names
                    .iter()
                    .any(|n| n.as_str() == "org.kde.StatusNotifierWatcher") =>
            {
                info!("StatusNotifierWatcher is available");
                return;
            }
            Ok(_) => {}
            Err(e) => warn!("D-Bus list_names failed: {e}"),
        }
        if start.elapsed() >= timeout {
            warn!(
                "StatusNotifierWatcher not available after {}s — proceeding anyway; \
                 ksni will re-register if it shows up later",
                timeout.as_secs()
            );
            return;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Apply a reading to the tray context and fire a notification if the
/// low-battery threshold was crossed.
fn handle_battery_update(
    ctx: &Arc<Mutex<BatteryContext>>,
    handle: &ksni::blocking::Handle<BatteryTray>,
    status: MouseStatus,
) {
    let threshold = config::low_battery_threshold();

    info!(
        "Battery: {}%{}",
        status.battery_level,
        if status.is_charging { " ⚡" } else { "" }
    );

    let should_notify = {
        let mut ctx_g = ctx.lock().unwrap();
        let previous = ctx_g.battery.map(|(l, _)| l).unwrap_or(100);
        ctx_g.battery = Some((status.battery_level, status.is_charging));
        ctx_g.notifications.should_notify_low_battery(
            status.battery_level,
            previous,
            threshold,
            status.is_charging,
        )
    };

    if should_notify {
        let mut ctx_g = ctx.lock().unwrap();
        if let Err(e) = ctx_g.notifications.send_low_battery(status.battery_level) {
            warn!("Failed to send low-battery notification: {e}");
        }
    }

    handle.update(|_| {});
}
