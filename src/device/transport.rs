use anyhow::{Context, Result};
use hidapi::{DeviceInfo, HidApi, HidDevice as RawHidDevice};
use std::ffi::CString;

const VID: u16 = 0x3710;
const PID_WIRED: u16 = 0x3414;
const PID_8K_DONGLE: u16 = 0x5406;

const INTERFACE: i32 = 1;

fn is_target(info: &DeviceInfo) -> bool {
    info.vendor_id() == VID
        && matches!(info.product_id(), PID_WIRED | PID_8K_DONGLE)
        && info.interface_number() == INTERFACE
}

/// Finds the mouse's HID node. The path is cached so a poll is a plain open
/// instead of a udev enumeration. It is re-checked on every open because
/// hidraw numbers get reassigned after a replug.
pub struct Locator {
    api: HidApi,
    path: Option<CString>,
}

impl Locator {
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: HidApi::new().context("Failed to initialize HID API")?,
            path: None,
        })
    }

    pub fn open(&mut self) -> Result<Device> {
        if let Some(path) = &self.path {
            if let Ok(device) = self.api.open_path(path)
                && device.get_device_info().is_ok_and(|info| is_target(&info))
            {
                return Ok(Device { device });
            }
            self.path = None;
        }

        self.api
            .reset_devices()
            .context("Failed to reset HID list")?;
        self.api
            .add_devices(VID, 0)
            .context("Failed to enumerate HID devices")?;
        let info = self
            .api
            .device_list()
            .find(|info| is_target(info))
            .context("Pulsar interface 1 not found")?;
        let path = info.path().to_owned();
        let device = info
            .open_device(&self.api)
            .context("Failed to open HID device")?;
        self.path = Some(path);
        Ok(Device { device })
    }
}

pub struct Device {
    device: RawHidDevice,
}

impl Device {
    /// One-off open with a fresh enumeration.
    pub fn open() -> Result<Self> {
        Locator::new()?.open()
    }

    pub fn write_output(&self, packet: &[u8; 17]) -> Result<()> {
        // Report 0x08 is 16 bytes plus the id, per the descriptor.
        self.device.write(packet).context("HID write failed")?;
        Ok(())
    }

    /// Discard queued input reports (the hidraw queue holds at most 64).
    pub fn drain_input(&self) {
        let mut buf = [0u8; 64];
        for _ in 0..64 {
            if !matches!(self.device.read_timeout(&mut buf, 0), Ok(n) if n > 0) {
                break;
            }
        }
    }

    /// One input report, or `None` on timeout.
    pub fn read_report(&self, timeout_ms: i32) -> Result<Option<Vec<u8>>> {
        let mut buf = [0u8; 64];

        let len = self
            .device
            .read_timeout(&mut buf, timeout_ms)
            .context("HID read failed")?;

        Ok((len > 0).then(|| buf[..len].to_vec()))
    }
}
