//! Vehicle parameters: download with gap fill, the QGroundControl `.params` text format, and
//! diffs between two snapshots.
use super::frame::{Msg, dialect, text};
use super::modes::Autopilot;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamValue {
    Int(i64),
    Real(f64),
}

impl ParamValue {
    /// Text as written in a `.params` file.
    pub fn to_text(self) -> String {
        match self {
            Self::Int(v) => v.to_string(),
            Self::Real(v) => format!("{v:.18}"),
        }
    }
    fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (a, b) => (a.as_f64() as f32).to_bits() == (b.as_f64() as f32).to_bits(),
        }
    }
    pub fn as_f64(self) -> f64 {
        match self {
            Self::Int(v) => v as f64,
            Self::Real(v) => v,
        }
    }
}

impl std::fmt::Display for ParamValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Real(v) => write!(f, "{}", *v as f32),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub value: ParamValue,
    /// MAV_PARAM_TYPE (1..=10).
    pub kind: u8,
}

/// A full parameter snapshot of one component.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamSet {
    pub system_id: u8,
    pub component_id: u8,
    /// Shown in the `.params` header ("PX4 Pro", "ArduPilot").
    pub stack: String,
    /// Shown in the `.params` header ("Multi-Rotor", …).
    pub vehicle: String,
    pub params: BTreeMap<String, Param>,
}

fn is_real(kind: u8) -> bool {
    matches!(kind, 9 | 10)
}

/// Decodes PARAM_VALUE.param_value: PX4 packs integers bytewise in the float, ArduPilot casts.
pub fn decode_value(raw: f32, kind: u8, autopilot: Autopilot) -> ParamValue {
    if is_real(kind) {
        return ParamValue::Real(raw as f64);
    }
    if autopilot != Autopilot::Px4 {
        return ParamValue::Int(raw as i64);
    }
    let bits = raw.to_bits();
    ParamValue::Int(match kind {
        1 => (bits & 0xff) as i64,
        2 => (bits & 0xff) as u8 as i8 as i64,
        3 => (bits & 0xffff) as i64,
        4 => (bits & 0xffff) as u16 as i16 as i64,
        5 => bits as i64,
        _ => bits as i32 as i64,
    })
}

impl ParamSet {
    /// QGroundControl's `.params` text (tab separated, `#` header).
    pub fn to_qgc(&self) -> String {
        let mut out = format!(
            "# Onboard parameters for Vehicle {}\n#\n# Stack: {}\n# Vehicle: {}\n#\n# Vehicle-Id Component-Id Name Value Type\n",
            self.system_id, self.stack, self.vehicle
        );
        for p in self.params.values() {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                self.system_id,
                self.component_id,
                p.name,
                p.value.to_text(),
                p.kind
            ));
        }
        out
    }
    /// Parses a `.params` file. Lines that don't parse are returned as errors by line number.
    pub fn from_qgc(textual: &str) -> (ParamSet, Vec<usize>) {
        let mut set = ParamSet::default();
        let mut bad = Vec::new();
        for (n, line) in textual.lines().enumerate() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("# Stack:") {
                set.stack = rest.trim().into();
            } else if let Some(rest) = line.strip_prefix("# Vehicle:") {
                set.vehicle = rest.trim().into();
            }
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            let parsed = (|| {
                let [sys, comp, name, value, kind] = fields.as_slice() else {
                    return None;
                };
                let kind: u8 = kind.parse().ok()?;
                let value = if is_real(kind) {
                    ParamValue::Real(value.parse().ok()?)
                } else {
                    ParamValue::Int(value.parse().ok()?)
                };
                Some((
                    sys.parse().ok()?,
                    comp.parse().ok()?,
                    name.to_string(),
                    value,
                    kind,
                ))
            })();
            match parsed {
                Some((sys, comp, name, value, kind)) => {
                    set.system_id = sys;
                    set.component_id = comp;
                    set.params.insert(name.clone(), Param { name, value, kind });
                }
                None => bad.push(n + 1),
            }
        }
        (set, bad)
    }
}

/// One difference between two snapshots.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamChange {
    pub name: String,
    pub before: Option<ParamValue>,
    pub after: Option<ParamValue>,
}

/// Parameters added, removed or changed from `before` to `after`, by name.
pub fn diff(before: &ParamSet, after: &ParamSet) -> Vec<ParamChange> {
    let mut changes = Vec::new();
    for (name, a) in &before.params {
        match after.params.get(name) {
            Some(b) if a.value.same(b.value) => {}
            other => changes.push(ParamChange {
                name: name.clone(),
                before: Some(a.value),
                after: other.map(|b| b.value),
            }),
        }
    }
    for (name, b) in &after.params {
        if !before.params.contains_key(name) {
            changes.push(ParamChange {
                name: name.clone(),
                before: None,
                after: Some(b.value),
            });
        }
    }
    changes.sort_by(|a, b| a.name.cmp(&b.name));
    changes
}

/// What a fetch wants next.
#[derive(Debug, PartialEq)]
pub enum FetchStep {
    Wait,
    Send(Vec<Msg>),
    Complete,
    /// Gave up; the indices still missing.
    Incomplete(Vec<u16>),
}

/// Downloads every parameter of one component: PARAM_REQUEST_LIST, then PARAM_REQUEST_READ by
/// index for gaps until complete or out of rounds.
pub struct ParamFetch {
    target: (u8, u8),
    autopilot: Autopilot,
    slots: Vec<Option<Param>>,
    received: usize,
    last_rx: Instant,
    list_requests: u32,
    rounds: u32,
    mark: usize,
    pub idle: Duration,
    pub max_rounds: u32,
    pub batch: usize,
}

impl ParamFetch {
    pub fn new(target: (u8, u8), autopilot: Autopilot, now: Instant) -> (Self, Msg) {
        let fetch = Self {
            target,
            autopilot,
            slots: Vec::new(),
            received: 0,
            last_rx: now,
            list_requests: 1,
            rounds: 0,
            mark: 0,
            idle: Duration::from_millis(700),
            max_rounds: 6,
            batch: 20,
        };
        let msg = fetch.request_list();
        (fetch, msg)
    }
    fn request_list(&self) -> Msg {
        Msg::PARAM_REQUEST_LIST(dialect::PARAM_REQUEST_LIST_DATA {
            target_system: self.target.0,
            target_component: self.target.1,
        })
    }
    pub fn progress(&self) -> (usize, usize) {
        (self.received, self.slots.len())
    }
    pub fn missing(&self) -> Vec<u16> {
        (0..self.slots.len())
            .filter(|&i| self.slots[i].is_none())
            .map(|i| i as u16)
            .collect()
    }
    /// Records a PARAM_VALUE from the target. Returns true when it was new.
    pub fn on_value(&mut self, v: &dialect::PARAM_VALUE_DATA, now: Instant) -> bool {
        self.last_rx = now;
        let total = v.param_count as usize;
        if total > self.slots.len() {
            self.slots.resize(total, None);
        }
        let name = text(&v.param_id);
        let index = if (v.param_index as usize) < self.slots.len() {
            Some(v.param_index as usize)
        } else {
            self.slots
                .iter()
                .position(|s| s.as_ref().is_some_and(|p| p.name == name))
        };
        let Some(index) = index else { return false };
        let kind = v.param_type as u8;
        let fresh = self.slots[index].is_none();
        self.slots[index] = Some(Param {
            value: decode_value(v.param_value, kind, self.autopilot),
            name,
            kind,
        });
        if fresh {
            self.received += 1;
        }
        fresh
    }
    pub fn is_complete(&self) -> bool {
        !self.slots.is_empty() && self.received == self.slots.len()
    }
    pub fn next_deadline(&self) -> Instant {
        self.last_rx + self.idle
    }
    pub fn poll(&mut self, now: Instant) -> FetchStep {
        if self.is_complete() {
            return FetchStep::Complete;
        }
        if now < self.next_deadline() {
            return FetchStep::Wait;
        }
        self.last_rx = now;
        if self.slots.is_empty() {
            if self.list_requests >= 3 {
                return FetchStep::Incomplete(Vec::new());
            }
            self.list_requests += 1;
            return FetchStep::Send(vec![self.request_list()]);
        }
        if self.received > self.mark {
            // Progress since the last round: the retry budget starts again.
            self.mark = self.received;
            self.rounds = 0;
        }
        if self.rounds >= self.max_rounds {
            return FetchStep::Incomplete(self.missing());
        }
        self.rounds += 1;
        let requests = self
            .missing()
            .into_iter()
            .take(self.batch)
            .map(|index| {
                Msg::PARAM_REQUEST_READ(dialect::PARAM_REQUEST_READ_DATA {
                    param_index: index as i16,
                    target_system: self.target.0,
                    target_component: self.target.1,
                    param_id: [0u8; 16].into(),
                })
            })
            .collect();
        FetchStep::Send(requests)
    }
    /// The parameters received so far, by name.
    pub fn into_set(self, stack: &str, vehicle: &str) -> ParamSet {
        ParamSet {
            system_id: self.target.0,
            component_id: self.target.1,
            stack: stack.into(),
            vehicle: vehicle.into(),
            params: self
                .slots
                .into_iter()
                .flatten()
                .map(|p| (p.name.clone(), p))
                .collect(),
        }
    }
}

/// `.params` header names for a vehicle.
pub fn header_names(autopilot: Autopilot, vehicle_type: u8) -> (&'static str, &'static str) {
    let stack = match autopilot {
        Autopilot::Px4 => "PX4 Pro",
        Autopilot::ArduPilot => "ArduPilot",
        Autopilot::Other(_) => "Generic",
    };
    let vehicle = match vehicle_type {
        1 => "Fixed Wing",
        10 => "Rover",
        11 => "Boat",
        12 => "Sub",
        19..=25 => "VTOL",
        2 | 3 | 4 | 13 | 14 | 15 | 29 => "Multi-Rotor",
        _ => "Generic",
    };
    (stack, vehicle)
}

/// The raw float a PARAM_SET carries for `value`: the inverse of [`decode_value`]. PX4 packs
/// integers into the float's bytes; ArduPilot sends them as float values.
pub fn encode_value(value: &ParamValue, kind: u8, autopilot: Autopilot) -> f32 {
    let i = match value {
        ParamValue::Real(r) => return *r as f32,
        ParamValue::Int(i) => *i,
    };
    if is_real(kind) || autopilot != Autopilot::Px4 {
        return i as f32;
    }
    f32::from_bits(match kind {
        1 | 2 => i as i8 as u8 as u32,
        3 | 4 => i as i16 as u16 as u32,
        5 => i as u32,
        _ => i as i32 as u32,
    })
}

fn param_type(kind: u8) -> dialect::MavParamType {
    use dialect::MavParamType as T;
    match kind {
        1 => T::MAV_PARAM_TYPE_UINT8,
        2 => T::MAV_PARAM_TYPE_INT8,
        3 => T::MAV_PARAM_TYPE_UINT16,
        4 => T::MAV_PARAM_TYPE_INT16,
        5 => T::MAV_PARAM_TYPE_UINT32,
        6 => T::MAV_PARAM_TYPE_INT32,
        7 => T::MAV_PARAM_TYPE_UINT64,
        8 => T::MAV_PARAM_TYPE_INT64,
        10 => T::MAV_PARAM_TYPE_REAL64,
        _ => T::MAV_PARAM_TYPE_REAL32,
    }
}

/// What a parameter write asks of the engine.
#[derive(Debug, PartialEq)]
pub enum WriteStep {
    Wait,
    Send(Msg),
    /// Every parameter was confirmed or given up on: names never confirmed are `failed`.
    Done {
        applied: usize,
        failed: Vec<String>,
    },
}

/// Writes parameters one at a time to an owned simulation (`Service::set_params` is refused on
/// any other link), each confirmed by the PARAM_VALUE the autopilot answers with. One that
/// never comes back with the value sent is tried again, then reported as not applied.
pub struct ParamWrite {
    target: (u8, u8),
    autopilot: Autopilot,
    queue: VecDeque<Param>,
    current: Option<(Param, Instant, u8)>,
    applied: usize,
    failed: Vec<String>,
    timeout: Duration,
    tries: u8,
}
impl ParamWrite {
    pub fn new(
        target: (u8, u8),
        autopilot: Autopilot,
        params: Vec<Param>,
        now: Instant,
    ) -> (Self, WriteStep) {
        let mut write = Self {
            target,
            autopilot,
            queue: params.into(),
            current: None,
            applied: 0,
            failed: Vec::new(),
            timeout: Duration::from_millis(800),
            tries: 3,
        };
        let step = write.next(now);
        (write, step)
    }
    fn set(&self, p: &Param) -> Msg {
        Msg::PARAM_SET(dialect::PARAM_SET_DATA {
            param_value: encode_value(&p.value, p.kind, self.autopilot),
            target_system: self.target.0,
            target_component: self.target.1,
            param_id: p.name.as_str().into(),
            param_type: param_type(p.kind),
        })
    }
    fn next(&mut self, now: Instant) -> WriteStep {
        match self.queue.pop_front() {
            Some(p) => {
                let msg = self.set(&p);
                self.current = Some((p, now, 1));
                WriteStep::Send(msg)
            }
            None => {
                self.current = None;
                WriteStep::Done {
                    applied: self.applied,
                    failed: std::mem::take(&mut self.failed),
                }
            }
        }
    }
    /// A PARAM_VALUE: when it confirms the parameter being written, the next one is sent.
    pub fn on_value(&mut self, v: &dialect::PARAM_VALUE_DATA, now: Instant) -> Option<WriteStep> {
        let (p, ..) = self.current.as_ref()?;
        let confirmed = text(&v.param_id) == p.name
            && v.param_value.to_bits() == encode_value(&p.value, p.kind, self.autopilot).to_bits();
        if !confirmed {
            return None;
        }
        self.applied += 1;
        Some(self.next(now))
    }
    pub fn poll(&mut self, now: Instant) -> WriteStep {
        let Some((p, sent, tries)) = self.current.as_mut() else {
            return WriteStep::Wait;
        };
        if now < *sent + self.timeout {
            return WriteStep::Wait;
        }
        if *tries < self.tries {
            *tries += 1;
            *sent = now;
            let p = p.clone();
            return WriteStep::Send(self.set(&p));
        }
        let name = p.name.clone();
        self.failed.push(name);
        self.next(now)
    }
    pub fn next_deadline(&self) -> Option<Instant> {
        self.current
            .as_ref()
            .map(|(_, sent, _)| *sent + self.timeout)
    }
}

#[cfg(test)]
#[path = "params_tests.rs"]
mod tests;
