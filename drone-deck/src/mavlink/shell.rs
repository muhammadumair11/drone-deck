//! The vehicle's text console (PX4 NuttShell / ArduPilot) over SERIAL_CONTROL, device SHELL.
use super::frame::{Msg, dialect};

/// SERIAL_CONTROL carries at most this many bytes.
pub const CHUNK: usize = 70;

fn message(target: (u8, u8), flags: dialect::SerialControlFlag, bytes: &[u8]) -> Msg {
    let mut data = [0u8; CHUNK];
    data[..bytes.len()].copy_from_slice(bytes);
    Msg::SERIAL_CONTROL(dialect::SERIAL_CONTROL_DATA {
        baudrate: 0,
        timeout: 0,
        device: dialect::SerialControlDev::SERIAL_CONTROL_DEV_SHELL,
        flags,
        count: bytes.len() as u8,
        data,
        target_system: target.0,
        target_component: target.1,
    })
}

/// Messages that type `text` into the shell (split into 70-byte chunks).
pub fn send(target: (u8, u8), text: &str) -> Vec<Msg> {
    let flags = dialect::SerialControlFlag::SERIAL_CONTROL_FLAG_RESPOND
        | dialect::SerialControlFlag::SERIAL_CONTROL_FLAG_EXCLUSIVE;
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return vec![message(target, flags, &[])];
    }
    bytes
        .chunks(CHUNK)
        .map(|c| message(target, flags, c))
        .collect()
}

/// The message that releases the shell (no EXCLUSIVE flag).
pub fn close(target: (u8, u8)) -> Msg {
    message(target, dialect::SerialControlFlag::empty(), &[])
}

/// Shell output carried by a SERIAL_CONTROL reply, if any.
pub fn output(data: &dialect::SERIAL_CONTROL_DATA) -> Option<&[u8]> {
    let shell = data.device == dialect::SerialControlDev::SERIAL_CONTROL_DEV_SHELL;
    let count = (data.count as usize).min(CHUNK);
    (shell && count > 0).then(|| &data.data[..count])
}
