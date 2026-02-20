use crate::keyboard::Keyboard;
use crate::protocols::zmk;
use crate::protocols::zmk_rpc;
use crate::protocols::{connect_protocol, parse_zmk_config, KeyboardDefinition, ZmkTransportConfig};
use crate::settings::{ProtocolType, Settings};
use std::sync::mpsc::{self, TryRecvError};

pub struct ConnectedState {
    pub settings: Settings,
    pub definition: KeyboardDefinition,
    pub layout_names: Vec<String>,
    pub keyboard: Keyboard,
}

pub struct ConnectionTask {
    rx: mpsc::Receiver<Result<ConnectedState, String>>,
}

impl ConnectionTask {
    pub fn start(settings: Settings) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = build_connected_state(settings);
            let _ = tx.send(result);
        });
        Self { rx }
    }

    pub fn try_finish(&self) -> Option<Result<ConnectedState, String>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("Background connection task failed".to_string()))
            }
        }
    }
}

pub fn build_connected_state(mut settings: Settings) -> Result<ConnectedState, String> {
    let protocol_config = settings.protocol_config.clone();

    let protocol = if settings.protocol_type == ProtocolType::Zmk {
        let (vid, pid, transport) =
            parse_zmk_config(&protocol_config).map_err(|e| format!("Invalid ZMK config: {e}"))?;

        let zmk_transport = match transport {
            ZmkTransportConfig::Serial(port_name) => zmk_rpc::ZmkTransport::SerialPort(port_name),
            ZmkTransportConfig::Ble(device_id) => zmk_rpc::ZmkTransport::BleDevice(device_id),
        };

        let zmk_data = zmk_rpc::fetch_zmk_data(&zmk_transport).map_err(|e| {
            if e.to_string() == "DEVICE_LOCKED" {
                "Device is locked. Please press the ZMK Studio unlock key combination on your keyboard, then click Connect again.".to_string()
            } else {
                format!("ZMK error: {e}")
            }
        })?;

        zmk::save_and_get_layout_names(vid, pid, &zmk_data)
            .map_err(|e| format!("Failed to process ZMK data: {e}"))?;

        connect_protocol(settings.protocol_type, &settings.protocol_config)
            .map_err(|e| format!("Failed to connect to device: {e}"))?
    } else {
        connect_protocol(settings.protocol_type, &settings.protocol_config)
            .map_err(|e| format!("Failed to connect to device: {e}"))?
    };

    let layout_names = protocol.get_layout_definition().get_layout_names();
    if let Some(first) = layout_names.first() {
        if !layout_names.contains(&settings.layout_name) {
            settings.layout_name = first.clone();
        }
    }
    let definition = protocol.get_layout_definition().clone();

    let keyboard = Keyboard::new(protocol, settings.layout_name.clone(), settings.timeout)
        .map_err(|e| format!("Failed to create keyboard: {e}"))?;

    Ok(ConnectedState {
        settings,
        definition,
        layout_names,
        keyboard,
    })
}
