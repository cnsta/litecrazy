use log::{debug, info, warn};
use std::sync::{
    Arc, Mutex,
    mpsc::{self, RecvTimeoutError},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::device::{
    protocol::{self, BatteryReadError, MouseStatus},
    transport::Locator,
};

#[derive(Debug, Clone)]
pub enum BatteryEvent {
    Update(MouseStatus),
    Asleep,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Refresh,
    GateChanged,
    Shutdown,
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

    /// The current state, expiring a timed pause whose deadline has passed.
    fn current(&self) -> GateState {
        let mut state = self.state.lock().unwrap();
        if let GateState::PausedUntil(deadline) = *state
            && Instant::now() >= deadline
        {
            info!("Poll pause expired; resuming battery polling");
            *state = GateState::Running;
        }
        *state
    }

    pub fn is_paused(&self) -> bool {
        self.current() != GateState::Running
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

    pub fn resume(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if matches!(*state, GateState::Running) {
            return false;
        }
        *state = GateState::Running;
        info!("Battery polling resumed");
        true
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
    pub gate: Arc<PollGate>,
}

/// Handle to the background worker. Dropping stops and joins the thread.
pub struct BatteryWorker {
    control: mpsc::Sender<Control>,
    join: Option<JoinHandle<()>>,
}

impl Drop for BatteryWorker {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl BatteryWorker {
    /// Spawn the worker thread. Returns the handle (drop to stop) and the
    /// receiving end of the event channel, which closes when the worker
    /// stops.
    pub fn spawn(config: WorkerConfig) -> (Self, mpsc::Receiver<BatteryEvent>) {
        let (tx, rx) = mpsc::channel();
        let (control, control_rx) = mpsc::channel();

        let join = thread::spawn(move || worker_loop(config, control_rx, tx));

        (
            Self {
                control,
                join: Some(join),
            },
            rx,
        )
    }

    /// A sender for waking or stopping the worker.
    pub fn control(&self) -> mpsc::Sender<Control> {
        self.control.clone()
    }
}

/// Outcome of one attempted poll.
#[derive(PartialEq, Debug)]
enum PollOutcome {
    Ok(MouseStatus),
    Asleep,
    Disconnected,
}

/// Device lookup state that survives between polls. The node itself is
/// opened per poll and closed again, so the kernel isn't queueing this
/// interface's reports for us in between.
struct Session {
    locator: Option<Locator>,
    handshaken: bool,
}

impl Session {
    fn poll(&mut self) -> PollOutcome {
        let locator = match &mut self.locator {
            Some(l) => l,
            None => match Locator::new() {
                Ok(l) => self.locator.insert(l),
                Err(e) => {
                    debug!("HID init failed: {e}");
                    return PollOutcome::Disconnected;
                }
            },
        };
        let device = match locator.open() {
            Ok(d) => d,
            Err(e) => {
                debug!("Device open failed: {e}");
                self.handshaken = false;
                return PollOutcome::Disconnected;
            }
        };

        match protocol::get_mouse_battery(&device, &mut self.handshaken) {
            Ok(status) => PollOutcome::Ok(status),
            Err(BatteryReadError::Asleep) => PollOutcome::Asleep,
            Err(BatteryReadError::Io(e)) => {
                info!("Transport error: {e}");
                self.handshaken = false;
                PollOutcome::Disconnected
            }
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

/// When the worker must wake on its own: the next poll while running, the
/// end of a timed pause, or never (only a `Control` message wakes it).
fn wake_deadline(gate: GateState, next_poll: Instant) -> Option<Instant> {
    match gate {
        GateState::Running => Some(next_poll),
        GateState::PausedUntil(t) => Some(t),
        GateState::Paused => None,
    }
}

fn worker_loop(
    config: WorkerConfig,
    control: mpsc::Receiver<Control>,
    tx: mpsc::Sender<BatteryEvent>,
) {
    let WorkerConfig {
        interval,
        disconnect_backoff,
        gate,
    } = config;

    info!(
        "Battery worker started (interval: {}s, disconnect_backoff: {}s)",
        interval.as_secs(),
        disconnect_backoff.as_secs(),
    );

    let mut session = Session {
        locator: None,
        handshaken: false,
    };
    let mut consecutive_asleep: u32 = 0;
    let mut next_poll = Instant::now();
    let mut was_paused = false;

    loop {
        let state = gate.current();
        if state == GateState::Running {
            // Poll immediately after a pause, so resuming from the menu
            // gives an instant reading rather than a stale one.
            if was_paused {
                was_paused = false;
                next_poll = Instant::now();
            }
            if Instant::now() >= next_poll {
                let delay = match session.poll() {
                    PollOutcome::Ok(status) => {
                        if consecutive_asleep > 0 {
                            debug!("Mouse awake after {consecutive_asleep} asleep poll(s)");
                        }
                        consecutive_asleep = 0;
                        let _ = tx.send(BatteryEvent::Update(status));
                        interval
                    }
                    PollOutcome::Asleep => {
                        consecutive_asleep = consecutive_asleep.saturating_add(1);
                        let delay = next_sleep_interval(interval, consecutive_asleep);
                        debug!(
                            "Mouse asleep (consecutive: {}, next poll in {}s)",
                            consecutive_asleep,
                            delay.as_secs()
                        );
                        let _ = tx.send(BatteryEvent::Asleep);
                        delay
                    }
                    PollOutcome::Disconnected => {
                        consecutive_asleep = 0;
                        warn!(
                            "Device unreachable, backing off {}s",
                            disconnect_backoff.as_secs()
                        );
                        let _ = tx.send(BatteryEvent::Disconnected);
                        disconnect_backoff
                    }
                };
                next_poll = Instant::now() + delay;
                continue;
            }
        } else {
            was_paused = true;
        }

        let msg = match wake_deadline(state, next_poll) {
            Some(t) => control.recv_timeout(t.saturating_duration_since(Instant::now())),
            None => control.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match msg {
            Ok(Control::Refresh) => next_poll = Instant::now(),
            Ok(Control::GateChanged) | Err(RecvTimeoutError::Timeout) => {}
            Ok(Control::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
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
    fn wake_deadline_follows_the_gate() {
        let poll = Instant::now() + Duration::from_secs(60);
        let pause_end = Instant::now() + Duration::from_secs(600);
        assert_eq!(wake_deadline(GateState::Running, poll), Some(poll));
        assert_eq!(
            wake_deadline(GateState::PausedUntil(pause_end), poll),
            Some(pause_end)
        );
        assert_eq!(wake_deadline(GateState::Paused, poll), None);
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
