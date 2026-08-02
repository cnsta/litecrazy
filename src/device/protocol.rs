use crate::device::transport::Device;

/// Battery request. The mouse replies with a report of the same command id.
const CMD04_PACKET: [u8; 17] = [
    0x08, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x49,
];

const CMD01_PACKET_A: [u8; 17] = [
    0x08, 0x01, 0x00, 0x00, 0x08, 0x8e, 0x0c, 0x4d, 0x4c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x11,
];
const CMD01_PACKET_B: [u8; 17] = [
    0x08, 0x01, 0x00, 0x00, 0x08, 0x95, 0x05, 0xdd, 0x4b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x82,
];
const CMD02_PACKET: [u8; 17] = [
    0x08, 0x02, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x49,
];
const CMD03_PACKET: [u8; 17] = [
    0x08, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x4A,
];

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

const MIN_REPLY_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseStatus {
    pub battery_level: u8,
    pub is_charging: bool,
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

fn read_cmd(device: &Device, expected_cmd: u8, timeout_ms: u64) -> Option<Vec<u8>> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

    while std::time::Instant::now() < deadline {
        match device.read_interrupt(250) {
            Ok(data) => {
                if data.len() < MIN_REPLY_LEN || data[0] != 0x08 || data[1] != expected_cmd {
                    continue;
                }
                return Some(data);
            }
            Err(_) => continue,
        }
    }

    None
}

fn parse_status(payload: &[u8]) -> MouseStatus {
    MouseStatus {
        // Clamp: the firmware has been seen to report >100 briefly while
        // charging, and the icon lookup indexes on this.
        battery_level: payload[6].min(100),
        is_charging: payload[7] != 0,
    }
}

pub fn get_mouse_battery(device: &Device) -> Result<MouseStatus, BatteryReadError> {
    device.drain_input(6);

    device
        .write_output(&CMD04_PACKET)
        .map_err(BatteryReadError::Io)?;

    if let Some(payload) = read_cmd(device, 0x04, 800) {
        return Ok(parse_status(&payload));
    }

    for pkt in &CMD04_INIT_SEQUENCE {
        device.write_output(pkt).map_err(BatteryReadError::Io)?;
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    if let Some(payload) = read_cmd(device, 0x04, 2000) {
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
