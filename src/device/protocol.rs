//! Report 0x08: `[0x08, cmd, status, 0, 0, len, data…, checksum]`, where the
//! bytes sum to 0x55 and the reply echoes `cmd` (status 1 = unsupported).
//! Command names come from Pulsar's web upgrader (bbb.pulsar.gg).

use crate::device::transport::Device;
use std::time::{Duration, Instant};

/// `EncryptionData`: four random bytes in, the reply carries device ids.
const CMD01_PACKET_A: [u8; 17] = [
    0x08, 0x01, 0x00, 0x00, 0x08, 0x8e, 0x0c, 0x4d, 0x4c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x11,
];
const CMD01_PACKET_B: [u8; 17] = [
    0x08, 0x01, 0x00, 0x00, 0x08, 0x95, 0x05, 0xdd, 0x4b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x82,
];
/// `PCDriverStatus`: tells the dongle a host driver is active.
const CMD02_PACKET: [u8; 17] = [
    0x08, 0x02, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x49,
];
/// `DeviceOnLine`: reply byte 6 is 1 while the mouse is linked to the dongle,
/// bytes 7..10 its pairing address.
const CMD03_PACKET: [u8; 17] = [
    0x08, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x4A,
];
/// Battery: reply byte 6 is the level (firmware steps of 5-10 %), 7 the
/// charging flag, 8..10 the cell voltage in mV (big endian).
const CMD04_PACKET: [u8; 17] = [
    0x08, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x49,
];

/// Handshake replayed from the vendor driver, needed before a freshly
/// plugged dongle answers battery requests.
const CMD04_INIT_SEQUENCE: [[u8; 17]; 18] = [
    CMD01_PACKET_A,
    CMD03_PACKET,
    CMD03_PACKET,
    CMD03_PACKET,
    CMD01_PACKET_A,
    CMD03_PACKET,
    CMD03_PACKET,
    CMD01_PACKET_A,
    CMD03_PACKET,
    CMD01_PACKET_B,
    CMD03_PACKET,
    CMD03_PACKET,
    CMD03_PACKET,
    CMD02_PACKET,
    CMD03_PACKET,
    CMD03_PACKET,
    CMD04_PACKET,
    CMD04_PACKET,
];

pub const ONLINE_QUERY: [u8; 17] = CMD03_PACKET;
pub const BATTERY_QUERY: [u8; 17] = CMD04_PACKET;

const MIN_REPLY_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseStatus {
    pub battery_level: u8,
    pub is_charging: bool,
    pub voltage_mv: Option<u16>,
}

#[derive(Debug)]
pub enum BatteryReadError {
    Asleep,
    Io(anyhow::Error),
}

impl std::fmt::Display for BatteryReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatteryReadError::Asleep => write!(f, "Mouse asleep (no reply to battery request)"),
            BatteryReadError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BatteryReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BatteryReadError::Asleep => None,
            BatteryReadError::Io(e) => e.source(),
        }
    }
}

impl From<anyhow::Error> for BatteryReadError {
    fn from(e: anyhow::Error) -> Self {
        BatteryReadError::Io(e)
    }
}

/// Wait for the reply to `expected_cmd`, skipping unrelated reports.
pub fn read_cmd(
    device: &Device,
    expected_cmd: u8,
    timeout_ms: u64,
) -> anyhow::Result<Option<Vec<u8>>> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(None);
        }
        let Some(data) = device.read_report(left.as_millis().min(250) as i32)? else {
            continue;
        };
        if data.len() >= MIN_REPLY_LEN && data[0] == 0x08 && data[1] == expected_cmd {
            return Ok(Some(data));
        }
    }
}

/// Send `packet` and wait for the reply carrying the same command id.
pub fn request(
    device: &Device,
    packet: &[u8; 17],
    timeout_ms: u64,
) -> anyhow::Result<Option<Vec<u8>>> {
    device.drain_input();
    device.write_output(packet)?;
    read_cmd(device, packet[1], timeout_ms)
}

pub(crate) fn parse_status(payload: &[u8]) -> MouseStatus {
    MouseStatus {
        // Clamp: the firmware has been seen to report >100 briefly while
        // charging, and the icon lookup indexes on this.
        battery_level: payload[6].min(100),
        is_charging: payload[7] != 0,
        voltage_mv: payload
            .get(8..10)
            .map(|v| u16::from_be_bytes([v[0], v[1]]))
            .filter(|&mv| mv != 0),
    }
}

/// `Some(linked)` from a `DeviceOnLine` reply, `None` if it isn't one we
/// understand.
pub(crate) fn parse_online(reply: &[u8]) -> Option<bool> {
    (reply[2] == 0).then(|| reply[6] != 0)
}

/// Read the battery. `handshaken` records whether the init sequence has run
/// for this dongle. Once it has, an offline mouse is reported as asleep
/// without the battery request's timeouts.
pub fn get_mouse_battery(
    device: &Device,
    handshaken: &mut bool,
) -> Result<MouseStatus, BatteryReadError> {
    let online = request(device, &CMD03_PACKET, 500)?
        .as_deref()
        .and_then(parse_online);
    if online == Some(false) && *handshaken {
        return Err(BatteryReadError::Asleep);
    }

    if let Some(payload) = request(device, &CMD04_PACKET, 800)? {
        return Ok(parse_status(&payload));
    }

    for pkt in &CMD04_INIT_SEQUENCE {
        device.write_output(pkt)?;
        std::thread::sleep(Duration::from_millis(10));
    }
    *handshaken = true;

    if let Some(payload) = read_cmd(device, 0x04, 2000)? {
        return Ok(parse_status(&payload));
    }

    Err(BatteryReadError::Asleep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_level_and_charging_flag() {
        let payload = [0x08, 0x04, 0, 0, 0, 0, 77, 0x01];
        let status = parse_status(&payload);
        assert_eq!(status.battery_level, 77);
        assert!(status.is_charging);
        assert_eq!(status.voltage_mv, None);
    }

    #[test]
    fn parses_discharging() {
        let payload = [0x08, 0x04, 0, 0, 0, 0, 42, 0x00];
        assert!(!parse_status(&payload).is_charging);
    }

    #[test]
    fn level_is_clamped_to_100() {
        let payload = [0x08, 0x04, 0, 0, 0, 0, 255, 0x01];
        assert_eq!(parse_status(&payload).battery_level, 100);
    }

    /// A real reply: 30 %, discharging, 3725 mV.
    #[test]
    fn parses_voltage() {
        let payload = [
            0x08, 0x04, 0, 0, 0, 0x02, 0x1e, 0x00, 0x0e, 0x8d, 0, 0, 0, 0, 0, 0, 0x8e,
        ];
        let status = parse_status(&payload);
        assert_eq!(status.battery_level, 30);
        assert_eq!(status.voltage_mv, Some(3725));
    }

    #[test]
    fn parses_online_reply() {
        let online = [
            0x08, 0x03, 0, 0, 0, 0x01, 0x01, 0xca, 0xc6, 0xba, 0, 0, 0, 0, 0, 0, 0xfe,
        ];
        assert_eq!(parse_online(&online), Some(true));
        let mut offline = online;
        offline[6] = 0;
        assert_eq!(parse_online(&offline), Some(false));
        let mut unsupported = online;
        unsupported[2] = 1;
        assert_eq!(parse_online(&unsupported), None);
    }

    #[test]
    fn static_packets_have_valid_checksums() {
        for pkt in [
            CMD01_PACKET_A,
            CMD01_PACKET_B,
            CMD02_PACKET,
            CMD03_PACKET,
            CMD04_PACKET,
        ] {
            let sum = pkt.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
            assert_eq!(sum, 0x55, "bad checksum in packet {pkt:02x?}");
        }
    }
}
