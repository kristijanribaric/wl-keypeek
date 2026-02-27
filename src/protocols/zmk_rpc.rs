use crate::layout_key::LayoutKey;
use crate::zmk_keycode_labels::behavior_to_layout_key;
use std::error::Error;
use std::io::{Read, Write};
use std::time::Duration;
use zmk_studio_api::proto::zmk::{core, keymap};
#[cfg(not(target_os = "windows"))]
use zmk_studio_api::transport::ble::BleDeviceInfo;
#[cfg(not(target_os = "windows"))]
use zmk_studio_api::transport::ble::BleTransport;
#[cfg(target_os = "windows")]
use zmk_studio_api::transport::winrt::WinRtGattTransport;
use zmk_studio_api::{Behavior, StudioClient};

pub struct ZmkSerialDevice {
    pub port_name: String,
    pub vid: u16,
    pub pid: u16,
    pub product: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZmkBleDevice {
    pub device_id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZmkTransport {
    SerialPort(String),
    BleDevice(String),
}

pub fn scan_serial_ports() -> Vec<ZmkSerialDevice> {
    let Ok(ports) = serialport::available_ports() else {
        return Vec::new();
    };

    ports
        .into_iter()
        .filter_map(|p| {
            if let serialport::SerialPortType::UsbPort(usb) = &p.port_type {
                Some(ZmkSerialDevice {
                    port_name: p.port_name,
                    vid: usb.vid,
                    pid: usb.pid,
                    product: usb.product.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(not(target_os = "windows"))]
pub fn scan_ble_devices() -> Result<Vec<ZmkBleDevice>, Box<dyn Error>> {
    let devices: Vec<BleDeviceInfo> =
        StudioClient::<zmk_studio_api::transport::ble::BleTransport>::list_ble_devices()?;
    Ok(devices
        .into_iter()
        .map(|device| {
            let display_name = device.display_name();
            ZmkBleDevice {
                device_id: device.device_id,
                display_name,
            }
        })
        .collect())
}

/// Probe a list of Bluetooth addresses and return those that are ZMK Studio
/// keyboards. Uses native WinRT GATT APIs so it works for devices that are
/// already paired and connected as HID peripherals (no advertisement scan).
#[cfg(target_os = "windows")]
pub fn probe_ble_devices(bt_addresses: &[[u8; 6]]) -> Vec<ZmkBleDevice> {
    StudioClient::<WinRtGattTransport>::probe_ble_devices(bt_addresses)
        .into_iter()
        .map(|info| {
            let display_name = info.display_name();
            let device_id = info.device_id;
            ZmkBleDevice {
                device_id,
                display_name,
            }
        })
        .collect()
}

pub struct ZmkData {
    pub physical_layouts: keymap::PhysicalLayouts,
    pub layout_keys: Vec<Vec<Option<LayoutKey>>>,
    pub layer_count: usize,
}

pub fn fetch_zmk_data(transport: &ZmkTransport) -> Result<ZmkData, Box<dyn Error>> {
    match transport {
        ZmkTransport::SerialPort(port_name) => {
            let client = StudioClient::open_serial(port_name)
                .map_err(|e| format!("Failed to open serial port '{}': {}", port_name, e))?;
            fetch_zmk_data_from_client(client)
        }
        ZmkTransport::BleDevice(device_id) => open_zmk_ble_and_fetch(device_id),
    }
}

fn open_zmk_ble_and_fetch(device_id: &str) -> Result<ZmkData, Box<dyn Error>> {
    // On Windows use the native WinRT transport which works for already-paired
    // BLE HID devices that are not currently advertising.
    #[cfg(target_os = "windows")]
    let client = StudioClient::<WinRtGattTransport>::open_ble_winrt(device_id)
        .map_err(|e| format!("Failed to connect to BLE device '{device_id}' via WinRT: {e}"))?;

    // On Linux / macOS use the btleplug-based transport (advertisement scan).
    #[cfg(not(target_os = "windows"))]
    let client = StudioClient::open_ble(device_id).map_err(|e| {
        format!(
            "Failed to open BLE device '{device_id}'. Make sure your adapter is enabled \
             and the board is advertising: {e}"
        )
    })?;

    fetch_zmk_data_from_client(client)
}

fn fetch_zmk_data_from_client<T: Read + Write>(
    mut client: StudioClient<T>,
) -> Result<ZmkData, Box<dyn Error>> {
    let lock_state = client.get_lock_state()?;
    if lock_state == core::LockState::ZmkStudioCoreLockStateLocked {
        drop(client);
        return Err("DEVICE_LOCKED".into());
    }

    let physical_layouts = client.get_physical_layouts()?;

    let resolved_layers: Vec<Vec<Behavior>> = client.resolve_keymap()?;
    let layer_count = resolved_layers.len();

    let layout_keys: Vec<Vec<Option<LayoutKey>>> = resolved_layers
        .iter()
        .map(|layer| layer.iter().map(behavior_to_layout_key).collect())
        .collect();

    // Drop the ZMK RPC connection and give transport time to settle before
    // the caller opens any other handle (e.g. HID).
    drop(client);
    std::thread::sleep(Duration::from_millis(100));

    Ok(ZmkData {
        physical_layouts,
        layout_keys,
        layer_count,
    })
}
