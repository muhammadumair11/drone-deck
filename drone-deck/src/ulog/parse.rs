//! ULog reader (PX4 file format version 1): header, flag bits, formats, info, multi info,
//! parameters, subscriptions, data, logged strings, dropouts and sync markers.
//!
//! The whole file stays in memory and data messages are indexed per subscription, so a
//! series is decoded only when asked for. Damage never panics: a cut-off tail marks the log
//! truncated, an unknown message type with a plausible header is skipped by its size (as
//! pyulog does), and anything else that does not parse skips forward to the next sync marker.
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

pub(crate) const MAGIC: [u8; 7] = *b"ULog\x01\x12\x35";
pub(crate) const SYNC: [u8; 8] = [0x2F, 0x73, 0x13, 0x20, 0x25, 0x0C, 0xBB, 0x12];
/// The only incompatible flag this reader understands: data appended after the log.
const DATA_APPENDED: u8 = 1;
/// Nested message types deeper than this are treated as a cycle.
const MAX_NESTING: usize = 8;
/// A data payload is at most `u16::MAX` bytes, so no format can be larger.
const MAX_LAYOUT: usize = u16::MAX as usize;
/// Array elements a format may expand to, so nested arrays of empty types cannot spin.
const MAX_STEPS: usize = 1 << 16;
/// An unknown message type is skipped by its size only when the size looks sane (pyulog
/// treats larger ones as corruption).
const MAX_UNKNOWN: usize = 10_000;

#[derive(Debug, PartialEq)]
pub(crate) enum Error {
    /// Shorter than the 16-byte header, or the magic is wrong.
    NotUlog,
    /// Flag bits the format says a reader must not ignore.
    Incompatible([u8; 8]),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotUlog => write!(f, "not a ULog file (missing ULog header)"),
            Error::Incompatible(flags) => write!(
                f,
                "this log uses incompatible ULog features this reader does not know ({flags:02x?})"
            ),
        }
    }
}

/// A primitive ULog field type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Base {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    Bool,
    Char,
}
impl Base {
    fn from_name(name: &str) -> Option<Base> {
        Some(match name {
            "int8_t" => Base::I8,
            "uint8_t" => Base::U8,
            "int16_t" => Base::I16,
            "uint16_t" => Base::U16,
            "int32_t" => Base::I32,
            "uint32_t" => Base::U32,
            "int64_t" => Base::I64,
            "uint64_t" => Base::U64,
            "float" => Base::F32,
            "double" => Base::F64,
            "bool" => Base::Bool,
            "char" => Base::Char,
            _ => return None,
        })
    }
    pub(crate) fn size(self) -> usize {
        match self {
            Base::I8 | Base::U8 | Base::Bool | Base::Char => 1,
            Base::I16 | Base::U16 => 2,
            Base::I32 | Base::U32 | Base::F32 => 4,
            Base::I64 | Base::U64 | Base::F64 => 8,
        }
    }
    /// Reads a little-endian value at `at`; `None` past the end of `b`.
    pub(crate) fn read(self, b: &[u8], at: usize) -> Option<f64> {
        let s = b.get(at..at.checked_add(self.size())?)?;
        Some(match self {
            Base::I8 => s[0] as i8 as f64,
            Base::U8 | Base::Char => s[0] as f64,
            Base::Bool => (s[0] != 0) as u8 as f64,
            Base::I16 => i16::from_le_bytes([s[0], s[1]]) as f64,
            Base::U16 => u16::from_le_bytes([s[0], s[1]]) as f64,
            Base::I32 => i32::from_le_bytes(s.try_into().ok()?) as f64,
            Base::U32 => u32::from_le_bytes(s.try_into().ok()?) as f64,
            Base::I64 => i64::from_le_bytes(s.try_into().ok()?) as f64,
            Base::U64 => u64::from_le_bytes(s.try_into().ok()?) as f64,
            Base::F32 => f32::from_le_bytes(s.try_into().ok()?) as f64,
            Base::F64 => f64::from_le_bytes(s.try_into().ok()?),
        })
    }
}

/// One field of a format definition as written: `float[3] xyz`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FieldDef {
    pub ty: String,
    pub array: Option<usize>,
    pub name: String,
}
/// A `F` message: `name:type field;type[n] field;…`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Format {
    pub name: String,
    pub fields: Vec<FieldDef>,
}
/// A decoded field: nested types flattened to `outer.inner`, arrays to `name[i]`; a char array
/// stays one text field of `len` bytes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Flat {
    pub name: String,
    pub base: Base,
    pub offset: usize,
    pub len: usize,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Layout {
    pub fields: Vec<Flat>,
    pub size: usize,
}
impl Layout {
    pub(crate) fn field(&self, name: &str) -> Option<&Flat> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// A logged topic instance (`A` message) and where its data messages are.
#[derive(Clone, Debug)]
pub(crate) struct Topic {
    pub name: String,
    pub instance: u8,
    /// `None` when the format or a nested type was never defined.
    pub layout: Option<Arc<Layout>>,
    /// Payload start (after the message id) and payload length of each data message.
    pub(crate) samples: Vec<(u32, u16)>,
}
impl Topic {
    pub(crate) fn len(&self) -> usize {
        self.samples.len()
    }
}

/// An info, multi-info or parameter value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}
impl Value {
    fn decode(ty: &str, raw: &[u8]) -> Value {
        let (base, array) = match ty.split_once('[') {
            Some((b, rest)) => (b, rest.trim_end_matches(']').parse::<usize>().ok()),
            None => (ty, None),
        };
        match (Base::from_name(base), array) {
            (Some(Base::Char), _) => Value::Text(
                String::from_utf8_lossy(raw)
                    .trim_end_matches('\0')
                    .to_string(),
            ),
            (Some(Base::F32 | Base::F64), None) => Base::from_name(base)
                .and_then(|b| b.read(raw, 0))
                .map_or(Value::Bytes(raw.to_vec()), Value::Float),
            (Some(b), None) => b
                .read(raw, 0)
                .map_or(Value::Bytes(raw.to_vec()), |v| Value::Int(v as i64)),
            _ => Value::Bytes(raw.to_vec()),
        }
    }
    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Float(v) => Some(*v),
            _ => None,
        }
    }
}
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(v) => write!(f, "{v}"),
            Value::Float(v) => write!(f, "{v}"),
            Value::Text(t) => write!(f, "{t}"),
            Value::Bytes(b) => write!(f, "{} bytes", b.len()),
        }
    }
}

/// A logged string (`L` or tagged `C`). Level follows syslog: 0 emergency … 3 error,
/// 4 warning, 5 notice, 6 info, 7 debug.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LogMessage {
    pub level: u8,
    pub timestamp: u64,
    pub text: String,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dropout {
    /// The last data timestamp seen before the gap.
    pub after_us: u64,
    pub ms: u16,
}
/// What the reader had to work around.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Issues {
    /// The last message was cut off.
    pub truncated: bool,
    /// Regions skipped to the next sync marker, and how many bytes that cost.
    pub corrupt_regions: usize,
    pub skipped_bytes: usize,
    /// Data for a message id nobody subscribed.
    pub orphan_data: usize,
    /// Messages of a type this reader does not know, skipped by their size.
    pub unknown_messages: usize,
    /// Subscriptions whose format could not be resolved.
    pub unresolved: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct Ulog {
    bytes: Vec<u8>,
    pub version: u8,
    /// Header timestamp (µs since boot).
    pub start_us: u64,
    pub appended: bool,
    pub formats: HashMap<String, Format>,
    pub topics: Vec<Topic>,
    pub info: BTreeMap<String, Value>,
    pub info_multi: BTreeMap<String, Vec<Value>>,
    pub params: BTreeMap<String, Value>,
    /// Parameter changes after the first data message: (timestamp, name, value).
    pub param_changes: Vec<(u64, String, Value)>,
    pub messages: Vec<LogMessage>,
    pub dropouts: Vec<Dropout>,
    pub syncs: usize,
    /// First and last data timestamps.
    pub first_us: Option<u64>,
    pub last_us: Option<u64>,
    pub issues: Issues,
}

fn key_split(key: &str) -> (&str, &str) {
    key.split_once(' ').unwrap_or(("", key))
}
fn parse_format(raw: &[u8]) -> Option<Format> {
    let text = String::from_utf8_lossy(raw);
    let (name, rest) = text.split_once(':')?;
    let mut fields = Vec::new();
    for part in rest.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (ty, fname) = part.split_once(' ')?;
        let (ty, array) = match ty.split_once('[') {
            Some((t, n)) => (t, Some(n.trim_end_matches(']').parse().ok()?)),
            None => (ty, None),
        };
        if array.is_some_and(|n: usize| n > MAX_LAYOUT) {
            return None;
        }
        fields.push(FieldDef {
            ty: ty.to_string(),
            array,
            name: fname.trim().to_string(),
        });
    }
    Some(Format {
        name: name.trim().to_string(),
        fields,
    })
}
/// Where `flatten` writes, and how much work it may still do.
struct Flattener<'a> {
    formats: &'a HashMap<String, Format>,
    out: Vec<Flat>,
    steps: usize,
}
impl Flattener<'_> {
    /// One more array element; `None` once a format expands beyond any real payload.
    fn step(&mut self, offset: usize) -> Option<()> {
        self.steps += 1;
        (self.steps <= MAX_STEPS && offset <= MAX_LAYOUT).then_some(())
    }
    fn flatten(&mut self, name: &str, prefix: &str, base: usize, depth: usize) -> Option<usize> {
        if depth > MAX_NESTING {
            return None;
        }
        let format = self.formats.get(name)?;
        let mut offset = base;
        for f in &format.fields {
            let count = f.array.unwrap_or(1);
            let full = format!("{prefix}{}", f.name);
            match Base::from_name(&f.ty) {
                Some(Base::Char) if f.array.is_some() => {
                    self.out.push(Flat {
                        name: full,
                        base: Base::Char,
                        offset,
                        len: count,
                    });
                    offset += count;
                    self.step(offset)?;
                }
                Some(base) => {
                    for i in 0..count {
                        let fname = match f.array {
                            Some(_) => format!("{full}[{i}]"),
                            None => full.clone(),
                        };
                        if !f.name.starts_with("_padding") {
                            self.out.push(Flat {
                                name: fname,
                                base,
                                offset,
                                len: 1,
                            });
                        }
                        offset += base.size();
                        self.step(offset)?;
                    }
                }
                None => {
                    for i in 0..count {
                        let p = match f.array {
                            Some(_) => format!("{full}[{i}]."),
                            None => format!("{full}."),
                        };
                        offset = self.flatten(&f.ty, &p, offset, depth + 1)?;
                        self.step(offset)?;
                    }
                }
            }
        }
        Some(offset)
    }
}
/// The flattened layout of format `name`; `None` when a type is missing, nesting is cyclic or
/// deeper than `MAX_NESTING`, or the format would be larger than any data message.
pub(crate) fn layout(formats: &HashMap<String, Format>, name: &str) -> Option<Layout> {
    let mut f = Flattener {
        formats,
        out: Vec::new(),
        steps: 0,
    };
    let size = f.flatten(name, "", 0, 0)?;
    Some(Layout {
        fields: f.out,
        size,
    })
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}
fn find_sync(b: &[u8], from: usize) -> Option<usize> {
    b.get(from..)?
        .windows(SYNC.len())
        .position(|w| w == SYNC)
        .map(|p| from + p)
}

/// Parses a complete or partial ULog file held in memory.
pub(crate) fn parse(bytes: Vec<u8>) -> Result<Ulog, Error> {
    if bytes.len() < 16 || bytes[..7] != MAGIC {
        return Err(Error::NotUlog);
    }
    let mut log = Ulog {
        version: bytes[7],
        start_us: u64_at(&bytes, 8).unwrap_or(0),
        appended: false,
        formats: HashMap::new(),
        topics: Vec::new(),
        info: BTreeMap::new(),
        info_multi: BTreeMap::new(),
        params: BTreeMap::new(),
        param_changes: Vec::new(),
        messages: Vec::new(),
        dropouts: Vec::new(),
        syncs: 0,
        first_us: None,
        last_us: None,
        issues: Issues::default(),
        bytes: Vec::new(),
    };
    let b = &bytes;
    let mut by_id: HashMap<u16, usize> = HashMap::new();
    // Offsets where appended data starts. The format lets a log stop mid-message there (a
    // crash dump appended to a log cut off by the crash), so no message may run across one.
    let mut appended_at: Vec<usize> = Vec::new();
    let mut pos = 16;
    while pos < b.len() {
        let next_append = appended_at.iter().copied().find(|&a| a > pos);
        let limit = next_append.unwrap_or(b.len());
        if pos + 3 > limit {
            match next_append {
                Some(a) => {
                    pos = a;
                    continue;
                }
                None => {
                    log.issues.truncated = true;
                    break;
                }
            }
        }
        let size = u16_at(b, pos).unwrap_or(0) as usize;
        let kind = b[pos + 2];
        let start = pos + 3;
        let end = start + size;
        if end > limit
            && let Some(a) = next_append
        {
            pos = a;
            continue;
        }
        let known = matches!(
            kind,
            b'B' | b'F'
                | b'I'
                | b'M'
                | b'P'
                | b'Q'
                | b'A'
                | b'R'
                | b'D'
                | b'L'
                | b'C'
                | b'S'
                | b'O'
        );
        let malformed = known
            && match kind {
                b'D' | b'R' | b'O' => size < 2,
                b'A' => size < 4,
                b'L' => size < 9,
                b'C' => size < 11,
                b'I' | b'P' => size < 1,
                b'M' | b'Q' => size < 2,
                b'B' => size < 40,
                _ => false,
            };
        let skippable = !known && kind.is_ascii_alphabetic() && (1..=MAX_UNKNOWN).contains(&size);
        // A size running past the end is a cut-off tail, unless a sync marker follows: then it
        // is a damaged header in the middle of the file.
        let overrun = end > limit;
        if (!known && !skippable) || malformed || (overrun && find_sync(b, start).is_some()) {
            // Jump to the appended data if the damage is before it, else to the next sync.
            let resume = next_append.or_else(|| find_sync(b, pos + 1).map(|s| s + SYNC.len()));
            log.issues.corrupt_regions += 1;
            match resume {
                Some(r) => {
                    log.issues.skipped_bytes += r - pos;
                    pos = r;
                    continue;
                }
                None => {
                    log.issues.skipped_bytes += b.len() - pos;
                    break;
                }
            }
        }
        if overrun {
            log.issues.truncated = true;
            break;
        }
        if skippable {
            log.issues.unknown_messages += 1;
            pos = end;
            continue;
        }
        let m = &b[start..end];
        match kind {
            b'B' => {
                let incompat: [u8; 8] = m[8..16].try_into().unwrap_or_default();
                if incompat[0] & !DATA_APPENDED != 0 || incompat[1..].iter().any(|&f| f != 0) {
                    return Err(Error::Incompatible(incompat));
                }
                log.appended = incompat[0] & DATA_APPENDED != 0;
                if log.appended {
                    appended_at = (0..3)
                        .filter_map(|k| u64_at(m, 16 + 8 * k))
                        .filter_map(|o| usize::try_from(o).ok())
                        .filter(|&o| o > end && o < b.len())
                        .collect();
                    appended_at.sort_unstable();
                }
            }
            b'F' => {
                if let Some(f) = parse_format(m) {
                    log.formats.insert(f.name.clone(), f);
                }
            }
            b'I' | b'P' | b'M' | b'Q' => {
                let skip = match kind {
                    b'M' | b'Q' => 1,
                    _ => 0,
                };
                let key_len = m[skip] as usize;
                let key = m
                    .get(skip + 1..skip + 1 + key_len)
                    .map(|k| String::from_utf8_lossy(k).to_string())
                    .unwrap_or_default();
                let (ty, name) = key_split(&key);
                let value = Value::decode(ty, m.get(skip + 1 + key_len..).unwrap_or_default());
                match kind {
                    _ if name.is_empty() => {}
                    b'I' => {
                        log.info.insert(name.to_string(), value);
                    }
                    b'P' => {
                        if let Some(t) = log.last_us {
                            log.param_changes.push((t, name.to_string(), value.clone()));
                        }
                        log.params.insert(name.to_string(), value);
                    }
                    b'M' => {
                        // `is_continued`: part of the previous value with the same key.
                        let continued = m[0] != 0;
                        let list = log.info_multi.entry(name.to_string()).or_default();
                        match (continued, list.last_mut(), &value) {
                            (true, Some(Value::Text(prev)), Value::Text(more)) => {
                                prev.push_str(more)
                            }
                            _ => list.push(value),
                        }
                    }
                    _ => {}
                }
            }
            b'A' => {
                let instance = m[0];
                let msg_id = u16_at(m, 1).unwrap_or(0);
                let name = String::from_utf8_lossy(&m[3..])
                    .trim_end_matches('\0')
                    .to_string();
                by_id.insert(msg_id, log.topics.len());
                log.topics.push(Topic {
                    name,
                    instance,
                    layout: None,
                    samples: Vec::new(),
                });
            }
            b'R' => {
                by_id.remove(&u16_at(m, 0).unwrap_or(0));
            }
            b'D' => {
                let msg_id = u16_at(m, 0).unwrap_or(0);
                match by_id.get(&msg_id) {
                    Some(&t) if start + 2 <= u32::MAX as usize => {
                        log.topics[t]
                            .samples
                            .push(((start + 2) as u32, (size - 2) as u16));
                        // PX4 writes `uint64_t timestamp` first in every topic; a zero one
                        // was never set and would stretch the duration back to boot.
                        if let Some(ts) = u64_at(m, 2).filter(|&t| t > 0) {
                            log.first_us = Some(log.first_us.map_or(ts, |f| f.min(ts)));
                            log.last_us = Some(log.last_us.map_or(ts, |l| l.max(ts)));
                        }
                    }
                    _ => log.issues.orphan_data += 1,
                }
            }
            b'L' | b'C' => {
                let (ts_at, text_at) = if kind == b'L' { (1, 9) } else { (3, 11) };
                let raw = m[0];
                log.messages.push(LogMessage {
                    level: if raw >= b'0' { raw - b'0' } else { raw }.min(7),
                    timestamp: u64_at(m, ts_at).unwrap_or(0),
                    text: String::from_utf8_lossy(&m[text_at..])
                        .trim_end_matches('\0')
                        .trim()
                        .to_string(),
                });
            }
            b'O' => log.dropouts.push(Dropout {
                after_us: log.last_us.unwrap_or(log.start_us),
                ms: u16_at(m, 0).unwrap_or(0),
            }),
            b'S' => log.syncs += 1,
            _ => {}
        }
        pos = end;
    }
    let mut cache: HashMap<String, Option<Arc<Layout>>> = HashMap::new();
    for t in &mut log.topics {
        let entry = cache
            .entry(t.name.clone())
            .or_insert_with(|| layout(&log.formats, &t.name).map(Arc::new));
        t.layout = entry.clone();
        if t.layout.is_none() && !log.issues.unresolved.contains(&t.name) {
            log.issues.unresolved.push(t.name.clone());
        }
    }
    log.bytes = bytes;
    Ok(log)
}
/// Samples of one field: timestamps (µs since boot) and values.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Series {
    pub t_us: Vec<u64>,
    pub v: Vec<f64>,
}
impl Series {
    pub(crate) fn is_empty(&self) -> bool {
        self.v.is_empty()
    }
}

impl Ulog {
    /// Topic instances with this name, in subscription order.
    pub(crate) fn topic(&self, name: &str) -> impl Iterator<Item = &Topic> {
        self.topics.iter().filter(move |t| t.name == name)
    }
    /// Instances of `name` that carry data, sorted.
    pub(crate) fn instances(&self, name: &str) -> Vec<u8> {
        let mut v: Vec<u8> = self
            .topic(name)
            .filter(|t| t.len() > 0)
            .map(|t| t.instance)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
    /// Several fields of one topic instance sampled together, sorted by timestamp. A sample
    /// whose payload is too short for a field is left out; fields missing from the format give
    /// `None`.
    pub(crate) fn series_many(
        &self,
        name: &str,
        instance: u8,
        fields: &[&str],
    ) -> Option<(Vec<u64>, Vec<Vec<f64>>)> {
        let mut rows: Vec<(u64, Vec<f64>)> = Vec::new();
        let mut found = false;
        for t in self.topic(name).filter(|t| t.instance == instance) {
            let Some(layout) = &t.layout else { continue };
            let Some(flats) = fields
                .iter()
                .map(|f| layout.field(f))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            found = true;
            let ts = layout.field("timestamp");
            for &(at, len) in &t.samples {
                let data = &self.bytes[at as usize..at as usize + len as usize];
                let time = ts.and_then(|f| u64_at(data, f.offset)).unwrap_or_default();
                let values: Option<Vec<f64>> =
                    flats.iter().map(|f| f.base.read(data, f.offset)).collect();
                if let Some(values) = values {
                    rows.push((time, values));
                }
            }
        }
        if !found {
            return None;
        }
        rows.sort_by_key(|r| r.0);
        let mut cols = vec![Vec::with_capacity(rows.len()); fields.len()];
        let mut t_us = Vec::with_capacity(rows.len());
        for (t, values) in rows {
            t_us.push(t);
            for (c, v) in cols.iter_mut().zip(values) {
                c.push(v);
            }
        }
        Some((t_us, cols))
    }
    /// One field of one topic instance, e.g. `series("vehicle_attitude", 0, "q[0]")`.
    pub(crate) fn series(&self, name: &str, instance: u8, field: &str) -> Option<Series> {
        let (t_us, mut cols) = self.series_many(name, instance, &[field])?;
        Some(Series {
            t_us,
            v: cols.pop().unwrap_or_default(),
        })
    }
    /// The first instance of `name` that has data and every field in `fields`.
    pub(crate) fn first_instance(&self, name: &str, fields: &[&str]) -> Option<u8> {
        self.instances(name).into_iter().find(|&i| {
            self.topic(name).any(|t| {
                t.instance == i
                    && t.layout
                        .as_ref()
                        .is_some_and(|l| fields.iter().all(|f| l.field(f).is_some()))
            })
        })
    }
    pub(crate) fn info_text(&self, key: &str) -> Option<String> {
        self.info.get(key).map(|v| v.to_string())
    }
}
