//! A small ULog writer and a synthetic flight built with it, for tests and the headless
//! `flight-logs` scene. Nothing here reads or represents a real vehicle.
use super::parse::{MAGIC, SYNC};

pub(crate) struct Writer {
    pub buf: Vec<u8>,
}
impl Writer {
    pub(crate) fn new(start_us: u64) -> Writer {
        let mut buf = MAGIC.to_vec();
        buf.push(1);
        buf.extend_from_slice(&start_us.to_le_bytes());
        Writer { buf }
    }
    pub(crate) fn msg(&mut self, kind: u8, payload: &[u8]) -> &mut Writer {
        self.buf
            .extend_from_slice(&(payload.len() as u16).to_le_bytes());
        self.buf.push(kind);
        self.buf.extend_from_slice(payload);
        self
    }
    /// Flag bits with the given incompatible flags and first appended-data offset.
    pub(crate) fn flags(&mut self, incompat0: u8, appended: u64) -> &mut Writer {
        let mut p = vec![0u8; 8];
        p.push(incompat0);
        p.extend_from_slice(&[0; 7]);
        p.extend_from_slice(&appended.to_le_bytes());
        p.extend_from_slice(&[0; 16]);
        self.msg(b'B', &p)
    }
    pub(crate) fn format(&mut self, def: &str) -> &mut Writer {
        self.msg(b'F', def.as_bytes())
    }
    fn keyed(&mut self, kind: u8, lead: &[u8], key: &str, value: &[u8]) -> &mut Writer {
        let mut p = lead.to_vec();
        p.push(key.len() as u8);
        p.extend_from_slice(key.as_bytes());
        p.extend_from_slice(value);
        self.msg(kind, &p)
    }
    pub(crate) fn info(&mut self, name: &str, value: &str) -> &mut Writer {
        let key = format!("char[{}] {name}", value.len());
        self.keyed(b'I', &[], &key, value.as_bytes())
    }
    pub(crate) fn info_u32(&mut self, name: &str, value: u32) -> &mut Writer {
        self.keyed(b'I', &[], &format!("uint32_t {name}"), &value.to_le_bytes())
    }
    pub(crate) fn info_multi(&mut self, name: &str, value: &str, continued: bool) -> &mut Writer {
        let key = format!("char[{}] {name}", value.len());
        self.keyed(b'M', &[continued as u8], &key, value.as_bytes())
    }
    pub(crate) fn param_f32(&mut self, name: &str, value: f32) -> &mut Writer {
        self.keyed(b'P', &[], &format!("float {name}"), &value.to_le_bytes())
    }
    pub(crate) fn param_i32(&mut self, name: &str, value: i32) -> &mut Writer {
        self.keyed(b'P', &[], &format!("int32_t {name}"), &value.to_le_bytes())
    }
    pub(crate) fn add(&mut self, instance: u8, msg_id: u16, name: &str) -> &mut Writer {
        let mut p = vec![instance];
        p.extend_from_slice(&msg_id.to_le_bytes());
        p.extend_from_slice(name.as_bytes());
        self.msg(b'A', &p)
    }
    #[cfg(test)]
    pub(crate) fn remove(&mut self, msg_id: u16) -> &mut Writer {
        self.msg(b'R', &msg_id.to_le_bytes())
    }
    /// A data message; `body` follows the timestamp.
    pub(crate) fn data(&mut self, msg_id: u16, t_us: u64, body: &[u8]) -> &mut Writer {
        let mut p = msg_id.to_le_bytes().to_vec();
        p.extend_from_slice(&t_us.to_le_bytes());
        p.extend_from_slice(body);
        self.msg(b'D', &p)
    }
    pub(crate) fn log(&mut self, level: u8, t_us: u64, text: &str) -> &mut Writer {
        let mut p = vec![b'0' + level];
        p.extend_from_slice(&t_us.to_le_bytes());
        p.extend_from_slice(text.as_bytes());
        self.msg(b'L', &p)
    }
    pub(crate) fn tagged(&mut self, level: u8, tag: u16, t_us: u64, text: &str) -> &mut Writer {
        let mut p = vec![b'0' + level];
        p.extend_from_slice(&tag.to_le_bytes());
        p.extend_from_slice(&t_us.to_le_bytes());
        p.extend_from_slice(text.as_bytes());
        self.msg(b'C', &p)
    }
    pub(crate) fn dropout(&mut self, ms: u16) -> &mut Writer {
        self.msg(b'O', &ms.to_le_bytes())
    }
    pub(crate) fn sync(&mut self) -> &mut Writer {
        self.msg(b'S', &SYNC)
    }
}

/// Little-endian packing for data bodies.
#[derive(Default)]
pub(crate) struct Body(pub Vec<u8>);
impl Body {
    pub(crate) fn f32(mut self, v: f32) -> Body {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub(crate) fn f64(mut self, v: f64) -> Body {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub(crate) fn u8(mut self, v: u8) -> Body {
        self.0.push(v);
        self
    }
    pub(crate) fn u16(mut self, v: u16) -> Body {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub(crate) fn u32(mut self, v: u32) -> Body {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
}

pub(crate) const LOCAL: u16 = 0;
pub(crate) const GLOBAL: u16 = 1;
pub(crate) const BATTERY0: u16 = 2;
pub(crate) const BATTERY1: u16 = 3;
pub(crate) const STATUS: u16 = 4;
pub(crate) const ATTITUDE: u16 = 5;
pub(crate) const IMU: u16 = 6;
pub(crate) const ESTIMATOR: u16 = 7;
/// Boot time of the first sample.
pub(crate) const T0: u64 = 5_000_000;

/// Position along the synthetic flight at `t` seconds: (east m, up m).
fn track(t: f64) -> (f64, f64) {
    match t {
        t if t < 10. => (0., 0.),
        t if t < 30. => (0., (t - 10.) * 1.),
        t if t < 50. => ((t - 30.) * 5., 20.),
        t if t < 70. => (100. - (t - 50.) * 5., 20.),
        t if t < 90. => (0., 20. - (t - 70.) * 1.),
        _ => (0., 0.),
    }
}

/// A 100 s synthetic flight sampled at 5 Hz: armed 10–92 s, climb to 20 m, 100 m out and back
/// at 5 m/s, a battery failsafe at 55 s, two battery instances, one dropout and three logged
/// messages.
pub(crate) fn flight() -> Vec<u8> {
    let mut w = Writer::new(T0 - 1_000);
    w.flags(0, 0)
        .format("vehicle_local_position:uint64_t timestamp;float x;float y;float z;float vx;float vy;float vz;bool xy_valid;bool z_valid;uint8_t[6] _padding0;")
        .format("vehicle_global_position:uint64_t timestamp;double lat;double lon;float alt;uint8_t[4] _padding0;")
        .format("battery_status:uint64_t timestamp;float voltage_v;float remaining;bool connected;uint8_t[7] _padding0;")
        .format("vehicle_status:uint64_t timestamp;uint8_t arming_state;bool failsafe;uint8_t nav_state;uint8_t[5] _padding0;")
        .format("vehicle_attitude:uint64_t timestamp;float[4] q;")
        .format("vehicle_imu_status:uint64_t timestamp;float accel_vibration_metric;uint8_t[4] _padding0;")
        .format("estimator_status:uint64_t timestamp;uint32_t filter_fault_flags;uint16_t innovation_check_flags;uint8_t[2] _padding0;")
        .info("sys_name", "PX4")
        .info("ver_hw", "PX4_SITL")
        .info("sys_os_name", "Linux")
        .info("ver_sw", "0123456789abcdef0123456789abcdef01234567")
        .info_u32("ver_sw_release", 0x010F_00FF)
        .info_multi("perf_counter_preflight", "sensors: 12 events, ", false)
        .info_multi("perf_counter_preflight", "ekf2: 3 events", true)
        .param_f32("MPC_XY_VEL_MAX", 12.)
        .param_i32("SYS_AUTOSTART", 10040)
        .add(0, LOCAL, "vehicle_local_position")
        .add(0, GLOBAL, "vehicle_global_position")
        .add(0, BATTERY0, "battery_status")
        .add(1, BATTERY1, "battery_status")
        .add(0, STATUS, "vehicle_status")
        .add(0, ATTITUDE, "vehicle_attitude")
        .add(0, IMU, "vehicle_imu_status")
        .add(0, ESTIMATOR, "estimator_status");
    let mut prev = track(0.);
    for k in 0..=500u64 {
        let t = k as f64 * 0.2;
        let ts = T0 + k * 200_000;
        let (e, u) = track(t);
        let (ve, vu) = ((e - prev.0) / 0.2, (u - prev.1) / 0.2);
        prev = (e, u);
        let armed = (10. ..92.).contains(&t);
        w.data(
            LOCAL,
            ts,
            &Body::default()
                .f32(0.)
                .f32(e as f32)
                .f32(-u as f32)
                .f32(0.)
                .f32(ve as f32)
                .f32(-vu as f32)
                .u8(1)
                .u8(1)
                .0,
        );
        w.data(
            GLOBAL,
            ts,
            &Body::default()
                .f64(47.397742)
                .f64(8.545594 + e / 75_000.)
                .f32(488. + u as f32)
                .0,
        );
        let used = if armed {
            (t - 10.) / 82.
        } else if t >= 92. {
            1.
        } else {
            0.
        };
        w.data(
            BATTERY0,
            ts,
            &Body::default()
                .f32((16.8 - 1.9 * used) as f32)
                .f32((1. - 0.65 * used) as f32)
                .u8(1)
                .0,
        );
        w.data(BATTERY1, ts, &Body::default().f32(0.).f32(-1.).u8(0).0);
        let failsafe = (55. ..70.).contains(&t);
        w.data(
            STATUS,
            ts,
            &Body::default()
                .u8(if armed { 2 } else { 1 })
                .u8(failsafe as u8)
                .u8(if failsafe { 5 } else { 4 })
                .0,
        );
        let roll = if (30. ..32.).contains(&t) { 0.2f64 } else { 0. };
        w.data(
            ATTITUDE,
            ts,
            &Body::default()
                .f32((roll / 2.).cos() as f32)
                .f32((roll / 2.).sin() as f32)
                .f32(0.)
                .f32(0.)
                .0,
        );
        w.data(
            IMU,
            ts,
            &Body::default()
                .f32(if armed {
                    4. + (t * 3.).sin() as f32
                } else {
                    0.1
                })
                .0,
        );
        w.data(
            ESTIMATOR,
            ts,
            &Body::default()
                .u32(0)
                .u16(if (40. ..41.).contains(&t) { 4 } else { 0 })
                .0,
        );
        if k == 50 {
            w.log(6, ts, "Armed by external command");
        }
        if k == 275 {
            w.log(4, ts, "Low battery level! Return advised").tagged(
                3,
                1,
                ts,
                "Failsafe activated: RTL",
            );
        }
        if k == 300 {
            w.dropout(120).sync();
        }
    }
    w.buf
}

/// 60 s of a synthetic PX4 quadrotor as the vehicle link records it: 1 Hz heartbeats (armed
/// from 10 s to 50 s, Position mode), a climb to 20 m and back, a battery that sags from 16.4 V,
/// and two STATUSTEXT lines, one of them MAVLink 1. Every frame carries a real CRC.
pub(crate) fn tlog() -> Vec<u8> {
    use crate::mavlink::{Encoder, Msg, dialect};
    let start = 1_793_000_000_000_000u64;
    let mut enc = Encoder::new(1, 1);
    let mut out = Vec::new();
    let record = |out: &mut Vec<u8>, t_us: u64, bytes: &[u8]| {
        out.extend_from_slice(&t_us.to_be_bytes());
        out.extend_from_slice(bytes);
    };
    for k in 0..=60u64 {
        let t = start + k * 1_000_000;
        let armed = (10..50).contains(&k);
        let mut flags = dialect::MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED;
        if armed {
            flags |= dialect::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED;
        }
        let heartbeat = Msg::HEARTBEAT(dialect::HEARTBEAT_DATA {
            custom_mode: 3 << 16,
            mavtype: dialect::MavType::MAV_TYPE_QUADROTOR,
            autopilot: dialect::MavAutopilot::MAV_AUTOPILOT_PX4,
            base_mode: flags,
            system_status: dialect::MavState::MAV_STATE_ACTIVE,
            mavlink_version: 3,
        });
        record(&mut out, t, &enc.encode(&heartbeat).bytes);
        let height = if armed {
            20_000 - (k as i32 - 30).abs() * 1_000
        } else {
            0
        };
        let position = Msg::GLOBAL_POSITION_INT(dialect::GLOBAL_POSITION_INT_DATA {
            time_boot_ms: k as u32 * 1000,
            lat: 473_977_420,
            lon: 85_455_940,
            alt: 488_000 + height,
            relative_alt: height,
            ..Default::default()
        });
        record(&mut out, t + 500_000, &enc.encode(&position).bytes);
        let battery = Msg::SYS_STATUS(dialect::SYS_STATUS_DATA {
            voltage_battery: 16_400 - k as u16 * 10,
            battery_remaining: 100 - k as i8,
            ..Default::default()
        });
        record(&mut out, t + 600_000, &enc.encode(&battery).bytes);
        let text = match k {
            12 => Some((
                dialect::MavSeverity::MAV_SEVERITY_INFO,
                "Takeoff detected",
                true,
            )),
            50 => Some((
                dialect::MavSeverity::MAV_SEVERITY_WARNING,
                "Low battery",
                false,
            )),
            _ => None,
        };
        if let Some((severity, text, v1)) = text {
            let msg = Msg::STATUSTEXT(dialect::STATUSTEXT_DATA {
                severity,
                text: text.into(),
                ..Default::default()
            });
            let frame = if v1 {
                enc.encode_v1(&msg)
            } else {
                enc.encode(&msg)
            };
            record(&mut out, t + 700_000, &frame.bytes);
        }
    }
    out
}
