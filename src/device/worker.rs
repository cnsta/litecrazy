use log::{debug, info, warn};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::device::{
    protocol::{self, BatteryReadError, MouseStatus},
    transport::Device,
};

#[derive(Debug, Clone)]
pub enum BatteryEvent {
    Update(MouseStatus),
    Asleep,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateState {
    Running,
    PausedUntil(Instant),
    Paused,
}

#[derive(Debug)]
pub struct PollGate {
    state: Mutex<GateState>,
}

impl Default for PollGate {
    fn default() -> Self {
        Self {
            state: Mutex::new(GateState::Running),
        }
    }
}

impl PollGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_paused(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if let GateState::PausedUntil(deadline) = *state {
            if Instant::now() >= deadline {
                info!("Poll pause expired; resuming battery polling");
                *state = GateState::Running;
            }
        }
        !matches!(*state, GateState::Running)
    }

    pub fn pause_for(&self, duration: Duration) {
        if duration.is_zero() {
            return;
        }
        let mut state = self.state.lock().unwrap();
        *state = GateState::PausedUntil(Instant::now() + duration);
        info!("Battery polling paused for {}s", duration.as_secs());
    }

    pub fn pause_indefinitely(&self) {
        *self.state.lock().unwrap() = GateState::Paused;
        info!("Battery polling paused");
    }

    pub fn resume(&self) {
        *self.state.lock().unwrap() = GateState::Running;
        info!("Battery polling resumed");
    }

    pub fn toggle(&self) {
        if self.is_paused() {
            self.resume();
        } else {
            self.pause_indefinitely();
        }
    }
}

pub struct WorkerConfig {
    pub interval: Duration,
    pub disconnect_backoff: Duration,
    pub refresh_flag: Arc<AtomicBool>,
    pub gate: Arc<PollGate>,
}

/// Handle to the background worker. Dropping joins the thread.
pub struct BatteryWorker {
    running: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for BatteryWorker {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl BatteryWorker {
    /// Spawn the worker thread. Returns the handle (drop to stop) and the
    /// receiving end of the event channel.
    pub fn spawn(config: WorkerConfig) -> (Self, mpsc::Receiver<BatteryEvent>) {
        let (tx, rx) = mpsc::channel();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let join = thread::spawn(move || worker_loop(config, running_clone, tx));

        (
            Self {
                running,
                join: Some(join),
            },
            rx,
        )
    }
}

/// Outcome of one attempted poll.
#[derive(PartialEq, Debug)]
enum PollOutcome {
    Ok(MouseStatus),
    Asleep,
    Disconnected,
}

fn poll_once() -> PollOutcome {
    let device = match Device::open() {
        Ok(d) => d,
        Err(e) => {
            debug!("Device::open failed: {e}");
            return PollOutcome::Disconnected;
        }
    };

    match protocol::get_mouse_battery(&device) {
        Ok(status) => PollOutcome::Ok(status),
        Err(BatteryReadError::Asleep) => PollOutcome::Asleep,
        Err(BatteryReadError::Io(e)) => {
            info!("Transport error: {e}");
            PollOutcome::Disconnected
        }
    }
}

fn next_sleep_interval(base: Duration, consecutive_asleep: u32) -> Duration {
    let multiplier: u32 = match consecutive_asleep {
        0 | 1 => 1,
        2 => 2,
        3 => 4,
        _ => 8,
    };
    base.saturating_mul(multiplier)
}

fn worker_loop(config: WorkerConfig, running: Arc<AtomicBool>, tx: mpsc::Sender<BatteryEvent>) {
    let WorkerConfig {
        interval,
        disconnect_backoff,
        refresh_flag,
        gate,
    } = config;

    info!(
        "Battery worker started (interval: {}s, disconnect_backoff: {}s)",
        interval.as_secs(),
        disconnect_backoff.as_secs(),
    );

    const TICK: Duration = Duration::from_millis(200);

    let mut accumulated = Duration::ZERO;
    let mut backoff_remaining = Duration::ZERO;
    let mut consecutive_asleep: u32 = 0;
    let mut current_interval = interval;
    let mut first_poll_pending = true;
    let mut was_paused = false;

    while running.load(Ordering::Acquire) {
        thread::sleep(TICK);

        let manual_refresh = refresh_flag.swap(false, Ordering::AcqRel);

        if gate.is_paused() {
            was_paused = true;
            continue;
        }

        // Poll immediately on the first tick after a pause, so resuming from
        // the menu gives an instant reading rather than a stale one.
        let just_resumed = was_paused;
        was_paused = false;

        if manual_refresh || just_resumed {
            backoff_remaining = Duration::ZERO;
        } else if backoff_remaining > Duration::ZERO {
            backoff_remaining = backoff_remaining.saturating_sub(TICK);
            continue;
        } else {
            accumulated += TICK;
            if !first_poll_pending && accumulated < current_interval {
                continue;
            }
        }

        accumulated = Duration::ZERO;
        first_poll_pending = false;

        match poll_once() {
            PollOutcome::Ok(status) => {
                if consecutive_asleep > 0 {
                    debug!("Mouse awake after {consecutive_asleep} asleep poll(s)");
                }
                consecutive_asleep = 0;
                current_interval = interval;
                let _ = tx.send(BatteryEvent::Update(status));
            }
            PollOutcome::Asleep => {
                consecutive_asleep = consecutive_asleep.saturating_add(1);
                current_interval = next_sleep_interval(interval, consecutive_asleep);
                debug!(
                    "Mouse asleep (consecutive: {}, next poll in {}s)",
                    consecutive_asleep,
                    current_interval.as_secs()
                );
                let _ = tx.send(BatteryEvent::Asleep);
            }
            PollOutcome::Disconnected => {
                consecutive_asleep = 0;
                current_interval = interval;
                backoff_remaining = disconnect_backoff;
                warn!(
                    "Device unreachable, backing off {}s",
                    disconnect_backoff.as_secs()
                );
                let _ = tx.send(BatteryEvent::Disconnected);
            }
        }
    }

    info!("Battery worker shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_interval_first_poll_uses_base() {
        let base = Duration::from_secs(60);
        assert_eq!(next_sleep_interval(base, 0), base);
        assert_eq!(next_sleep_interval(base, 1), base);
    }

    #[test]
    fn sleep_interval_doubles() {
        let base = Duration::from_secs(60);
        assert_eq!(next_sleep_interval(base, 2), Duration::from_secs(120));
        assert_eq!(next_sleep_interval(base, 3), Duration::from_secs(240));
    }

    #[test]
    fn sleep_interval_caps_at_eight_times() {
        let base = Duration::from_secs(60);
        assert_eq!(next_sleep_interval(base, 4), Duration::from_secs(480));
        assert_eq!(next_sleep_interval(base, 100), Duration::from_secs(480));
    }

    #[test]
    fn gate_starts_running() {
        assert!(!PollGate::new().is_paused());
    }

    #[test]
    fn gate_pauses_and_resumes() {
        let gate = PollGate::new();
        gate.pause_indefinitely();
        assert!(gate.is_paused());
        gate.resume();
        assert!(!gate.is_paused());
    }

    #[test]
    fn gate_toggles() {
        let gate = PollGate::new();
        gate.toggle();
        assert!(gate.is_paused());
        gate.toggle();
        assert!(!gate.is_paused());
    }

    #[test]
    fn zero_duration_pause_is_a_noop() {
        let gate = PollGate::new();
        gate.pause_for(Duration::ZERO);
        assert!(!gate.is_paused());
    }

    #[test]
    fn timed_pause_expires_on_its_own() {
        let gate = PollGate::new();
        gate.pause_for(Duration::from_millis(50));
        assert!(gate.is_paused());
        thread::sleep(Duration::from_millis(80));
        assert!(!gate.is_paused());
    }
}
