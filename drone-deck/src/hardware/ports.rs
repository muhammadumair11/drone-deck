//! Serial ports for a physical vehicle's endpoint, listed on a worker thread with their USB
//! product names, and the endpoint a chosen port, baud rate or network preset makes. The
//! endpoint is chosen, not typed: the field stays for an address no button offers.
use crate::mavlink::Endpoint;
use crate::*;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Baud rates flight controllers and telemetry radios commonly use: 57600 for SiK radios,
/// 115200 for a USB connection, the faster ones for companion links.
pub(crate) const BAUDS: [u32; 4] = [57_600, 115_200, 921_600, 1_500_000];
/// The baud a newly chosen port starts with.
pub(crate) const DEFAULT_BAUD: u32 = 115_200;
/// Common network endpoints: a UDP listener for telemetry radios and companion computers,
/// and the TCP server of Wi-Fi telemetry bridges.
pub(crate) const PRESETS: [(&str, &str); 2] = [
    ("UDP 14550, listen", "udp:0.0.0.0:14550"),
    ("TCP 192.168.4.1:5760, Wi-Fi bridge", "tcp:192.168.4.1:5760"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Port {
    pub name: String,
    /// The USB product and maker, or the kind of port.
    pub detail: String,
}

/// The last scan and the one running.
#[derive(Default)]
pub(crate) struct Scan {
    /// `None` until a scan finished.
    pub found: Option<Vec<Port>>,
    pub error: String,
    rx: Option<Receiver<Result<Vec<Port>, String>>>,
}
impl Scan {
    pub(crate) fn scanning(&self) -> bool {
        self.rx.is_some()
    }
}

pub(crate) fn endpoint(port: &str, baud: u32) -> String {
    format!("serial:{port}:{baud}")
}

/// The port and baud of a serial endpoint.
pub(crate) fn serial_of(text: &str) -> Option<(String, u32)> {
    match text.parse::<Endpoint>() {
        Ok(Endpoint::Serial { port, baud }) => Some((port, baud)),
        _ => None,
    }
}

fn describe(kind: &serialport::SerialPortType) -> String {
    match kind {
        serialport::SerialPortType::UsbPort(usb) => {
            let names: Vec<&str> = [usb.product.as_deref(), usb.manufacturer.as_deref()]
                .into_iter()
                .flatten()
                .filter(|n| !n.trim().is_empty())
                .collect();
            if names.is_empty() {
                format!("USB {:04x}:{:04x}", usb.vid, usb.pid)
            } else {
                names.join(" / ")
            }
        }
        serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
        serialport::SerialPortType::PciPort => "Built-in".into(),
        serialport::SerialPortType::Unknown => "Serial port".into(),
    }
}

/// Lists serial ports off the UI thread; `poll` lands the result.
pub(crate) fn scan(s: &mut State) {
    if s.headless {
        s.hardware.message = "Preview: the serial scan is disabled.".into();
        return;
    }
    if s.hardware.ports.scanning() {
        return;
    }
    let (tx, rx) = mpsc::channel();
    let waker = s.vehicle.waker.clone();
    std::thread::spawn(move || {
        let result = serialport::available_ports()
            .map(|ports| {
                ports
                    .into_iter()
                    .map(|p| Port {
                        detail: describe(&p.port_type),
                        name: p.port_name,
                    })
                    .collect()
            })
            .map_err(|e| format!("Serial ports could not be listed: {e}"));
        let _ = tx.send(result);
        if let Some(wake) = waker {
            wake();
        }
    });
    s.hardware.ports.rx = Some(rx);
}

pub(crate) fn poll(s: &mut State) -> bool {
    let scan = &mut s.hardware.ports;
    let Some(rx) = &scan.rx else {
        return false;
    };
    let result = match rx.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => return false,
        Err(TryRecvError::Disconnected) => {
            Err("The serial scan stopped before it finished.".into())
        }
    };
    scan.rx = None;
    match result {
        Ok(ports) => {
            scan.error.clear();
            scan.found = Some(ports);
        }
        Err(e) => scan.error = e,
    }
    true
}
