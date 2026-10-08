use crate::device::{protocol, Device};

pub fn run() -> anyhow::Result<()> {
    let device = Device::open()?;

    match protocol::request(&device, &protocol::ONLINE_QUERY, 800)? {
        Some(reply) => println!(
            "online  (0x03): {}  linked={:?}",
            hex(&reply),
            protocol::parse_online(&reply)
        ),
        None => println!("online  (0x03): no reply"),
    }

    match protocol::request(&device, &protocol::BATTERY_QUERY, 800)? {
        Some(reply) => println!(
            "battery (0x04): {}  {:?}",
            hex(&reply),
            protocol::parse_status(&reply)
        ),
        None => println!("battery (0x04): no reply (asleep, or the dongle needs the handshake)"),
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
