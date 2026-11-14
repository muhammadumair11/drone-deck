//! DataFlash reader: ArduPilot's binary flight log (`.bin`).
//!
//! A log is a stream of messages, each `0xA3 0x95 <type>` and a payload laid out by an earlier
//! FMT message (type 128) that names the type, its total length, its field types and its column
//! names. FMTU and MULT messages say, per column, by how much a raw value is scaled.
//!
//! Values follow pymavlink's reading: `c`, `C`, `e`, `E` are hundredths and `L` is 1e-7 degrees.
//! A column of another numeric type whose FMTU multiplier is not 1 (Copter 4.6 logs guided targets
//! in centimetres) is scaled to its unit as well, so callers read metres and metres per second.
//!
//! Only the message types a caller asks for are kept. Damage never panics: bytes that do not start
//! a message are skipped to the next header, and a cut-off last message is dropped.
use std::collections::HashMap;
use std::fmt;

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

const HEAD: [u8; 2] = [0xA3, 0x95];
const FMT: u8 = 128;
/// FMT's own payload: type, length, name[4], format[16], columns[64].
const FMT_LEN: usize = 3 + 1 + 1 + 4 + 16 + 64;

#[derive(Debug, PartialEq)]
pub(crate) enum Error {
    /// No FMT message where a DataFlash log starts.
    NotDataFlash,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotDataFlash => write!(f, "not an ArduPilot DataFlash log (no FMT message)"),
        }
    }
}

/// One field value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Int(i64),
    Float(f64),
    Text(String),
    /// `a`: int16[32].
    Ints(Vec<i64>),
}
impl Value {
    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub(crate) fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            _ => None,
        }
    }
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t),
            _ => None,
        }
    }
}

/// A message type as its FMT message describes it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Format {
    pub name: String,
    pub types: Vec<u8>,
    pub columns: Vec<String>,
    /// Total message length, header included.
    pub length: usize,
}
impl Format {
    pub(crate) fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }
}

/// The kept messages of a log, by name, in log order.
#[derive(Debug, Default)]
pub(crate) struct Log {
    pub formats: HashMap<String, Format>,
    rows: HashMap<String, Vec<Vec<Value>>>,
    /// Bytes skipped because they did not start a known message.
    pub skipped: usize,
}

/// One kept message: its values by column name.
pub(crate) struct Row<'a> {
    format: &'a Format,
    values: &'a [Value],
}
impl Row<'_> {
    pub(crate) fn get(&self, column: &str) -> Option<&Value> {
        self.format.column(column).and_then(|i| self.values.get(i))
    }
    pub(crate) fn f64(&self, column: &str) -> Option<f64> {
        self.get(column).and_then(Value::as_f64)
    }
    pub(crate) fn i64(&self, column: &str) -> Option<i64> {
        self.get(column).and_then(Value::as_i64)
    }
    pub(crate) fn text(&self, column: &str) -> Option<&str> {
        self.get(column).and_then(Value::as_str)
    }
    /// `TimeUS`, the time since boot (µs).
    pub(crate) fn time_us(&self) -> Option<u64> {
        self.i64("TimeUS").and_then(|t| u64::try_from(t).ok())
    }
}

impl Log {
    /// The kept messages named `name`, in log order.
    pub(crate) fn rows(&self, name: &str) -> impl Iterator<Item = Row<'_>> {
        let format = self.formats.get(name);
        self.rows
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(move |values| {
                format.map(|format| Row {
                    format,
                    values: values.as_slice(),
                })
            })
    }
    pub(crate) fn count(&self, name: &str) -> usize {
        self.rows.get(name).map_or(0, Vec::len)
    }
}

/// The size of a field type, or `None` for a type this reader does not know.
fn size(t: u8) -> Option<usize> {
    Some(match t {
        b'b' | b'B' | b'M' => 1,
        b'h' | b'H' | b'c' | b'C' => 2,
        b'i' | b'I' | b'f' | b'e' | b'E' | b'L' | b'n' => 4,
        b'd' | b'q' | b'Q' => 8,
        b'N' => 16,
        b'Z' | b'a' => 64,
        _ => return None,
    })
}

fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

fn decode(t: u8, b: &[u8]) -> Value {
    let a2 = |b: &[u8]| [b[0], b[1]];
    let a4 = |b: &[u8]| [b[0], b[1], b[2], b[3]];
    let a8 = |b: &[u8]| [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
    match t {
        b'b' => Value::Int(b[0] as i8 as i64),
        b'B' | b'M' => Value::Int(b[0] as i64),
        b'h' => Value::Int(i16::from_le_bytes(a2(b)) as i64),
        b'H' => Value::Int(u16::from_le_bytes(a2(b)) as i64),
        b'c' => Value::Float(i16::from_le_bytes(a2(b)) as f64 / 100.0),
        b'C' => Value::Float(u16::from_le_bytes(a2(b)) as f64 / 100.0),
        b'i' => Value::Int(i32::from_le_bytes(a4(b)) as i64),
        b'I' => Value::Int(u32::from_le_bytes(a4(b)) as i64),
        b'e' => Value::Float(i32::from_le_bytes(a4(b)) as f64 / 100.0),
        b'E' => Value::Float(u32::from_le_bytes(a4(b)) as f64 / 100.0),
        b'L' => Value::Float(i32::from_le_bytes(a4(b)) as f64 * 1e-7),
        b'f' => Value::Float(f32::from_le_bytes(a4(b)) as f64),
        b'd' => Value::Float(f64::from_le_bytes(a8(b))),
        b'q' => Value::Int(i64::from_le_bytes(a8(b))),
        b'Q' => Value::Int(u64::from_le_bytes(a8(b)).min(i64::MAX as u64) as i64),
        b'a' => Value::Ints(
            b.as_chunks::<2>()
                .0
                .iter()
                .map(|c| i16::from_le_bytes(*c) as i64)
                .collect(),
        ),
        _ => Value::Text(text(b)),
    }
}

/// Types already scaled by their format character; FMTU multipliers do not apply to them.
fn self_scaled(t: u8) -> bool {
    matches!(t, b'c' | b'C' | b'e' | b'E' | b'L')
}

/// ArduPilot's multiplier ids (`log_Multipliers`), for logs that name them without MULT.
fn known_multiplier(id: u8) -> Option<f64> {
    Some(match id {
        b'2' => 1e2,
        b'1' => 1e1,
        b'0' | b'?' => 1.0,
        b'A' => 1e-1,
        b'B' => 1e-2,
        b'C' => 1e-3,
        b'D' => 1e-4,
        b'E' => 1e-5,
        b'F' => 1e-6,
        b'G' => 1e-7,
        b'I' => 1e-9,
        b'!' => 3.6,
        b'/' => 3600.0,
        _ => return None,
    })
}

/// Reads `bytes`, keeping the messages named in `wanted` (and the formats of every type).
pub(crate) fn parse(bytes: &[u8], wanted: &[&str]) -> Result<Log, Error> {
    let start = bytes
        .windows(3)
        .take(4096)
        .position(|w| w[..2] == HEAD && w[2] == FMT)
        .ok_or(Error::NotDataFlash)?;
    let mut log = Log {
        skipped: start,
        ..Default::default()
    };
    let mut by_type: HashMap<u8, Format> = HashMap::new();
    // FMTU: type → per-column multiplier ids; MULT: id → multiplier.
    let mut units: HashMap<u8, Vec<u8>> = HashMap::new();
    let mut mults: HashMap<u8, f64> = HashMap::new();
    let mut raw: Vec<(u8, Vec<Value>)> = Vec::new();
    let mut at = start;
    while at + 3 <= bytes.len() {
        if bytes[at..at + 2] != HEAD {
            at += 1;
            log.skipped += 1;
            continue;
        }
        let kind = bytes[at + 2];
        if kind == FMT {
            if at + FMT_LEN > bytes.len() {
                break;
            }
            let p = &bytes[at + 3..at + FMT_LEN];
            let types: Vec<u8> = p[6..22].iter().copied().take_while(|b| *b != 0).collect();
            let length = p[1] as usize;
            let payload: Option<usize> = types.iter().map(|t| size(*t)).sum();
            if payload.is_some_and(|n| n + 3 == length) {
                let format = Format {
                    name: text(&p[2..6]),
                    columns: text(&p[22..86]).split(',').map(str::to_string).collect(),
                    types,
                    length,
                };
                by_type.insert(p[0], format.clone());
                log.formats.insert(format.name.clone(), format);
            }
            at += FMT_LEN;
            continue;
        }
        let Some(format) = by_type.get(&kind) else {
            at += 1;
            log.skipped += 1;
            continue;
        };
        if at + format.length > bytes.len() {
            break;
        }
        let name = format.name.as_str();
        if !(wanted.contains(&name) || name == "MULT" || name == "FMTU") {
            at += format.length;
            continue;
        }
        let mut values = Vec::with_capacity(format.types.len());
        let mut offset = at + 3;
        for t in &format.types {
            let n = size(*t).expect("checked when the format was read");
            values.push(decode(*t, &bytes[offset..offset + n]));
            offset += n;
        }
        match name {
            "MULT" => {
                if let (Some(Value::Int(id)), Some(Value::Float(m))) =
                    (values.get(1), values.get(2))
                {
                    mults.insert(*id as u8, *m);
                }
            }
            "FMTU" => {
                if let (Some(Value::Int(t)), Some(Value::Text(ids))) =
                    (values.get(1), values.get(3))
                {
                    units.insert(*t as u8, ids.bytes().collect());
                }
            }
            name if wanted.contains(&name) => raw.push((kind, values)),
            _ => {}
        }
        at += format.length;
    }
    for (kind, mut values) in raw {
        let format = &by_type[&kind];
        if let Some(ids) = units.get(&kind) {
            for ((value, t), id) in values.iter_mut().zip(&format.types).zip(ids) {
                let m = mults.get(id).copied().or_else(|| known_multiplier(*id));
                if let (false, Some(m), Some(v)) = (self_scaled(*t), m, value.as_f64())
                    && m != 1.0
                    && m != 0.0
                    && !matches!(*t, b'Q' | b'q')
                {
                    *value = Value::Float(v * m);
                }
            }
        }
        log.rows
            .entry(format.name.clone())
            .or_default()
            .push(values);
    }
    Ok(log)
}
