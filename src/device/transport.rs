use anyhow::{Context, Result};
use hidapi::{HidApi, HidDevice as RawHidDevice};

const VID: u16 = 0x3710;
const PID_WIRED: u16 = 0x3414;
const PID_8K_DONGLE: u16 = 0x5406;

const INTERFACE: i32 = 1;

pub struct Device {
    device: RawHidDevice,
}

impl Device {
    pub fn open() -> Result<Self> {
        let api = HidApi::new().context("Failed to initialize HID API")?;

        let info = api
            .device_list()
            .find(|dev| {
                dev.vendor_id() == VID
                    && (dev.product_id() == PID_WIRED || dev.product_id() == PID_8K_DONGLE)
                    && dev.interface_number() == INTERFACE
            })
            .context("Pulsar interface 1 not found")?;

        let device = info
            .open_device(&api)
            .context("Failed to open HID device")?;

        Ok(Self { device })
    }

    pub fn write_output(&self, packet: &[u8; 17]) -> Result<()> {
        let mut report = [0u8; 65];
        report[0] = packet[0];
        report[1..17].copy_from_slice(&packet[1..]);
        self.device.write(&report).context("HID write failed")?;
        Ok(())
    }

    pub fn drain_input(&self, attempts: usize) {
        for _ in 0..attempts {
            let mut buf = [0u8; 64];
            let _ = self.device.read_timeout(&mut buf, 1);
        }
    }

    pub fn read_interrupt(&self, timeout_ms: i32) -> Result<Vec<u8>> {
        let mut buf = [0u8; 64];

        let len = self
            .device
            .read_timeout(&mut buf, timeout_ms)
            .context("HID read failed")?;

        if len == 0 {
            anyhow::bail!("Read timeout");
        }

        Ok(buf[..len].to_vec())
    }
}
