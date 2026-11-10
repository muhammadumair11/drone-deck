//! MAVLink framing: split a byte stream (or one datagram) into raw frames, validate their CRC
//! for messages the dialect knows, decode them, and encode outgoing messages. Raw frames are
//! kept as bytes so they can be forwarded and logged unchanged.
use mavlink::{
    MAVLinkV1MessageRaw, MAVLinkV2MessageRaw, MavHeader, MavlinkVersion, Message, calculate_crc,
};
use std::collections::HashMap;

pub type Msg = ::mavlink::dialects::ardupilotmega::MavMessage;
pub use ::mavlink::dialects::ardupilotmega as dialect;

const STX_V1: u8 = 0xFE;
const STX_V2: u8 = 0xFD;
const SIGNED: u8 = 0x01;
const SIGNATURE_LEN: usize = 13;
/// Longest possible frame (MAVLink 2, signed, 255-byte payload).
pub const MAX_FRAME: usize = 10 + 255 + 2 + SIGNATURE_LEN;

/// One complete MAVLink frame, as received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub bytes: Vec<u8>,
}

impl Frame {
    pub fn version(&self) -> MavlinkVersion {
        if self.bytes[0] == STX_V2 {
            MavlinkVersion::V2
        } else {
            MavlinkVersion::V1
        }
    }
    fn header_len(&self) -> usize {
        if self.bytes[0] == STX_V2 { 10 } else { 6 }
    }
    #[cfg(test)]
    pub fn sequence(&self) -> u8 {
        if self.bytes[0] == STX_V2 {
            self.bytes[4]
        } else {
            self.bytes[2]
        }
    }
    pub fn system_id(&self) -> u8 {
        if self.bytes[0] == STX_V2 {
            self.bytes[5]
        } else {
            self.bytes[3]
        }
    }
    pub fn component_id(&self) -> u8 {
        if self.bytes[0] == STX_V2 {
            self.bytes[6]
        } else {
            self.bytes[4]
        }
    }
    pub fn message_id(&self) -> u32 {
        if self.bytes[0] == STX_V2 {
            u32::from_le_bytes([self.bytes[7], self.bytes[8], self.bytes[9], 0])
        } else {
            self.bytes[5] as u32
        }
    }
    pub fn payload(&self) -> &[u8] {
        let start = self.header_len();
        &self.bytes[start..start + self.bytes[1] as usize]
    }
    /// The decoded message, or `None` for messages outside the dialect or malformed payloads.
    pub fn decode(&self) -> Option<Msg> {
        Msg::parse(self.version(), self.message_id(), self.payload()).ok()
    }
}

/// Whether `byte` starts a MAVLink 1 or 2 frame.
pub fn is_start(byte: u8) -> bool {
    byte == STX_V1 || byte == STX_V2
}

/// Total frame length implied by a header at the start of `buf`, if enough bytes are present.
pub fn frame_len(buf: &[u8]) -> Option<usize> {
    match *buf.first()? {
        STX_V1 => Some(buf.get(1).map(|&l| 6 + l as usize + 2)?),
        STX_V2 => {
            let len = *buf.get(1)? as usize;
            let flags = *buf.get(2)?;
            let signature = if flags & SIGNED != 0 {
                SIGNATURE_LEN
            } else {
                0
            };
            Some(10 + len + 2 + signature)
        }
        _ => None,
    }
}

/// Message ids whose dialect lookup is remembered.
const KNOWN_CACHE: usize = 512;

/// Incremental frame splitter. Keeps at most one partial frame between calls.
#[derive(Default)]
pub struct Framer {
    buf: Vec<u8>,
    known: HashMap<u32, bool>,
    /// Frames dropped for a bad checksum or unsupported flags.
    pub bad_frames: u64,
    /// Bytes skipped while looking for a start marker.
    pub skipped_bytes: u64,
}

impl Framer {
    pub fn new() -> Self {
        Self::default()
    }
    /// Discards a partial frame (datagram transports call this after each packet).
    pub fn reset(&mut self) {
        self.buf.clear();
    }
    fn is_known(&mut self, id: u32) -> bool {
        if let Some(&known) = self.known.get(&id) {
            return known;
        }
        let known = Msg::default_message_from_id(id).is_some();
        // Ids are 24-bit and line noise yields random ones: cache only a bounded set.
        if self.known.len() < KNOWN_CACHE {
            self.known.insert(id, known);
        }
        known
    }
    /// Appends bytes and moves every complete frame into `out`.
    pub fn push(&mut self, data: &[u8], out: &mut Vec<Frame>) {
        self.buf.extend_from_slice(data);
        let mut start = 0;
        loop {
            let Some(offset) = self.buf[start..].iter().position(|&b| is_start(b)) else {
                self.skipped_bytes += (self.buf.len() - start) as u64;
                start = self.buf.len();
                break;
            };
            self.skipped_bytes += offset as u64;
            start += offset;
            let rest = &self.buf[start..];
            if rest[0] == STX_V2 && rest.len() >= 3 && rest[2] & !SIGNED != 0 {
                self.bad_frames += 1;
                start += 1;
                continue;
            }
            let Some(len) = frame_len(rest) else { break };
            if rest.len() < len {
                break;
            }
            let frame = Frame {
                bytes: rest[..len].to_vec(),
            };
            let id = frame.message_id();
            if self.is_known(id) && !crc_ok(&frame, Msg::extra_crc(id)) {
                self.bad_frames += 1;
                start += 1;
                continue;
            }
            out.push(frame);
            start += len;
        }
        self.buf.drain(..start);
        if self.buf.len() > MAX_FRAME * 4 {
            // Unreachable with well-formed input; bounds memory against garbage.
            self.skipped_bytes += self.buf.len() as u64;
            self.buf.clear();
        }
    }
}

fn crc_ok(frame: &Frame, extra: u8) -> bool {
    let header = frame.header_len();
    let end = header + frame.bytes[1] as usize;
    let crc = calculate_crc(&frame.bytes[1..end], extra);
    frame.bytes[end..end + 2] == crc.to_le_bytes()
}

/// Encodes outgoing messages as MAVLink 2 with this station's ids and a running sequence.
pub struct Encoder {
    pub system_id: u8,
    pub component_id: u8,
    sequence: u8,
}

impl Encoder {
    pub fn new(system_id: u8, component_id: u8) -> Self {
        Self {
            system_id,
            component_id,
            sequence: 0,
        }
    }
    pub fn encode(&mut self, msg: &Msg) -> Frame {
        let header = self.header();
        let mut raw = MAVLinkV2MessageRaw::new();
        raw.serialize_message(header, msg);
        Frame {
            bytes: raw.raw_bytes().to_vec(),
        }
    }
    /// The same as MAVLink 1, which older radios and logs still carry.
    pub fn encode_v1(&mut self, msg: &Msg) -> Frame {
        let header = self.header();
        let mut raw = MAVLinkV1MessageRaw::new();
        raw.serialize_message(header, msg);
        Frame {
            bytes: raw.raw_bytes().to_vec(),
        }
    }
    fn header(&mut self) -> MavHeader {
        let header = MavHeader {
            system_id: self.system_id as u32,
            component_id: self.component_id,
            sequence: self.sequence,
        };
        self.sequence = self.sequence.wrapping_add(1);
        header
    }
}

/// Reads a NUL-padded MAVLink string field.
pub fn text<const N: usize>(chars: &mavlink::types::CharArray<N>) -> String {
    let bytes: &[u8] = &chars[..];
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;
