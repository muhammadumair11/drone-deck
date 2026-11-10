//! A scripted fake PX4 vehicle on loopback sockets, for service tests.
use super::frame::{Encoder, Framer, Msg, dialect};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// A clean directory under the test executable's `workspace` folder.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(std::env::temp_dir)
        .join("workspace")
        .join("mavlink-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A free loopback UDP port for a UDP-listen service, and a guard to hold for the test. The
/// port is released before the service binds it, so two tests in parallel could be handed
/// the same one and each service would hear the other test's vehicle: holding the guard runs
/// such tests one at a time.
pub fn udp_listen_port() -> (std::sync::MutexGuard<'static, ()>, u16) {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let port = UdpSocket::bind("127.0.0.1:0")
        .and_then(|s| s.local_addr())
        .expect("free port")
        .port();
    (guard, port)
}

/// Waits up to `limit` for `check` to return Some.
pub fn wait_for<T>(limit: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let end = Instant::now() + limit;
    loop {
        if let Some(v) = check() {
            return Some(v);
        }
        if Instant::now() > end {
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// What the fake does; tests change it while it runs.
pub struct Script {
    pub heartbeats: bool,
    /// MAV_RESULT for every COMMAND_LONG; None = never acknowledge.
    pub ack: Option<dialect::MavResult>,
    pub params: Vec<(String, f32, dialect::MavParamType)>,
    /// Indices left out of the answer to PARAM_REQUEST_LIST (still answered by index).
    pub drop_params: HashSet<u16>,
    pub log: Vec<u8>,
    /// LOG_DATA stops at this offset while set (an interrupted link).
    pub log_cut: Option<u32>,
    /// Offsets whose chunk is lost once.
    pub log_drop_once: HashSet<u32>,
    /// HOME_POSITION `(lat, lon in 1e-7 degrees, altitude MSL in mm)`; None = no home yet.
    pub home: Option<(i32, i32, i32)>,
    /// Answers MAV_CMD_REQUEST_MESSAGE; false = refuses it as unsupported (older firmware),
    /// leaving only MAV_CMD_GET_HOME_POSITION.
    pub request_message: bool,
    /// Sent once, in order, after the next telemetry burst.
    pub extra: Vec<Msg>,
    /// Everything the fake received.
    pub received: Vec<Msg>,
}

impl Default for Script {
    fn default() -> Self {
        let params = (0..50)
            .map(|i| {
                if i % 2 == 0 {
                    (
                        format!("PARAM_{i:02}"),
                        f32::from_bits(i as u32 * 3),
                        dialect::MavParamType::MAV_PARAM_TYPE_INT32,
                    )
                } else {
                    (
                        format!("PARAM_{i:02}"),
                        i as f32 * 0.5,
                        dialect::MavParamType::MAV_PARAM_TYPE_REAL32,
                    )
                }
            })
            .collect();
        Self {
            heartbeats: true,
            ack: Some(dialect::MavResult::MAV_RESULT_ACCEPTED),
            params,
            drop_params: HashSet::new(),
            log: (0..20_000u32).map(|i| (i * 7 % 251) as u8).collect(),
            log_cut: None,
            log_drop_once: HashSet::new(),
            home: Some((473_977_000, 85_456_000, 498_000)),
            request_message: true,
            extra: Vec::new(),
            received: Vec::new(),
        }
    }
}

enum Wire {
    Tcp(TcpStream),
    Udp(UdpSocket, SocketAddr),
}

impl Wire {
    fn send(&mut self, bytes: &[u8]) {
        let _ = match self {
            Self::Tcp(s) => s.write_all(bytes),
            Self::Udp(s, to) => s.send_to(bytes, *to).map(|_| ()),
        };
    }
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        match self {
            Self::Tcp(s) => s.read(buf).ok().filter(|&n| n > 0),
            Self::Udp(s, _) => s.recv_from(buf).ok().map(|(n, _)| n),
        }
    }
}

pub struct FakeVehicle {
    pub script: Arc<Mutex<Script>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// TCP server address (TCP mode).
    pub addr: SocketAddr,
}

/// PX4 custom mode for Position.
pub const POSITION_MODE: u32 = 3 << 16;

/// The service's one read request at link-up: MAV_CMD_REQUEST_MESSAGE for HOME_POSITION, or
/// its MAV_CMD_GET_HOME_POSITION fallback. Tests that check "nothing was commanded" skip it.
pub fn is_home_request(m: &Msg) -> bool {
    #[allow(deprecated)]
    let get_home = dialect::MavCmd::MAV_CMD_GET_HOME_POSITION;
    matches!(m, Msg::COMMAND_LONG(c)
        if (c.command == dialect::MavCmd::MAV_CMD_REQUEST_MESSAGE && c.param1 == 242.0)
            || c.command == get_home)
}

impl FakeVehicle {
    /// Serves one TCP client on an ephemeral loopback port.
    pub fn tcp() -> Self {
        Self::tcp_at(([127, 0, 0, 1], 0).into())
    }
    /// A TCP vehicle on `addr` (port 0 picks one), for a test that frees a port first and
    /// starts listening on it later, like the router does.
    pub fn tcp_at(addr: SocketAddr) -> Self {
        let listener = TcpListener::bind(addr).expect("bind");
        let addr = listener.local_addr().expect("addr");
        Self::spawn(addr, move || {
            let (stream, _) = listener.accept().ok()?;
            stream
                .set_read_timeout(Some(Duration::from_millis(20)))
                .ok()?;
            Some(Wire::Tcp(stream))
        })
    }
    /// Sends UDP to `service` from an ephemeral loopback port.
    pub fn udp(service: SocketAddr) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        socket
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("timeout");
        let addr = socket.local_addr().expect("addr");
        Self::spawn(addr, move || Some(Wire::Udp(socket, service)))
    }
    fn spawn(addr: SocketAddr, open: impl FnOnce() -> Option<Wire> + Send + 'static) -> Self {
        let script = Arc::new(Mutex::new(Script::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (script.clone(), stop.clone());
        let thread = thread::spawn(move || {
            if let Some(wire) = open() {
                run(wire, s, st);
            }
        });
        Self {
            script,
            stop,
            thread: Some(thread),
            addr,
        }
    }
    pub fn with<T>(&self, f: impl FnOnce(&mut Script) -> T) -> T {
        f(&mut self.script.lock().expect("script"))
    }
    pub fn received(&self, pick: impl Fn(&Msg) -> bool) -> Vec<Msg> {
        self.with(|s| s.received.iter().filter(|m| pick(m)).cloned().collect())
    }
}

impl Drop for FakeVehicle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn telemetry() -> Vec<Msg> {
    vec![
        Msg::HEARTBEAT(dialect::HEARTBEAT_DATA {
            custom_mode: POSITION_MODE,
            mavtype: dialect::MavType::MAV_TYPE_QUADROTOR,
            autopilot: dialect::MavAutopilot::MAV_AUTOPILOT_PX4,
            base_mode: dialect::MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED,
            system_status: dialect::MavState::MAV_STATE_STANDBY,
            mavlink_version: 3,
        }),
        Msg::ATTITUDE(dialect::ATTITUDE_DATA {
            roll: 0.1,
            pitch: -0.2,
            yaw: 1.0,
            ..Default::default()
        }),
        Msg::GLOBAL_POSITION_INT(dialect::GLOBAL_POSITION_INT_DATA {
            lat: 473_977_420,
            lon: 85_455_940,
            alt: 488_000,
            relative_alt: 12_500,
            vx: 300,
            vy: 400,
            vz: -100,
            hdg: 9_000,
            ..Default::default()
        }),
        Msg::SYS_STATUS(dialect::SYS_STATUS_DATA {
            onboard_control_sensors_present: dialect::MavSysStatusSensor::from_bits_truncate(0b111),
            onboard_control_sensors_enabled: dialect::MavSysStatusSensor::from_bits_truncate(0b111),
            onboard_control_sensors_health: dialect::MavSysStatusSensor::from_bits_truncate(0b011),
            voltage_battery: 16_200,
            battery_remaining: 87,
            ..Default::default()
        }),
        Msg::GPS_RAW_INT(dialect::GPS_RAW_INT_DATA {
            fix_type: dialect::GpsFixType::GPS_FIX_TYPE_3D_FIX,
            satellites_visible: 14,
            eph: 80,
            ..Default::default()
        }),
        Msg::EXTENDED_SYS_STATE(dialect::EXTENDED_SYS_STATE_DATA {
            landed_state: dialect::MavLandedState::MAV_LANDED_STATE_ON_GROUND,
            ..Default::default()
        }),
    ]
}

fn param_value(script: &Script, index: usize) -> Msg {
    let (name, value, kind) = &script.params[index];
    Msg::PARAM_VALUE(dialect::PARAM_VALUE_DATA {
        param_value: *value,
        param_count: script.params.len() as u16,
        param_index: index as u16,
        param_id: name.as_str().into(),
        param_type: *kind,
    })
}

fn run(mut wire: Wire, script: Arc<Mutex<Script>>, stop: Arc<AtomicBool>) {
    let mut enc = Encoder::new(1, 1);
    let mut framer = Framer::new();
    let mut buf = [0u8; 4096];
    let mut next_beat = Instant::now();
    let send = |wire: &mut Wire, enc: &mut Encoder, msg: &Msg| wire.send(&enc.encode(msg).bytes);
    let status = Msg::STATUSTEXT(dialect::STATUSTEXT_DATA {
        severity: dialect::MavSeverity::MAV_SEVERITY_INFO,
        text: "Fake vehicle ready".into(),
        ..Default::default()
    });
    let mut status = Some(status);
    while !stop.load(Ordering::Relaxed) {
        if Instant::now() >= next_beat {
            next_beat = Instant::now() + Duration::from_millis(100);
            if script.lock().expect("script").heartbeats {
                let extra = std::mem::take(&mut script.lock().expect("script").extra);
                for msg in telemetry().iter().chain(&extra) {
                    send(&mut wire, &mut enc, msg);
                }
                if let Some(status) = status.take() {
                    send(&mut wire, &mut enc, &status);
                }
            }
        }
        let Some(n) = wire.recv(&mut buf) else {
            continue;
        };
        let mut frames = Vec::new();
        framer.push(&buf[..n], &mut frames);
        for frame in frames {
            let Some(msg) = frame.decode() else { continue };
            let mut s = script.lock().expect("script");
            s.received.push(msg.clone());
            let mut out = Vec::new();
            match &msg {
                Msg::COMMAND_LONG(c) => {
                    use dialect::MavCmd::*;
                    let request = c.command == MAV_CMD_REQUEST_MESSAGE;
                    let result = if request && !s.request_message {
                        Some(dialect::MavResult::MAV_RESULT_UNSUPPORTED)
                    } else {
                        s.ack
                    };
                    if let Some(result) = result {
                        out.push(Msg::COMMAND_ACK(dialect::COMMAND_ACK_DATA {
                            command: c.command,
                            result,
                            target_system: frame.system_id(),
                            target_component: frame.component_id(),
                            ..Default::default()
                        }));
                    }
                    #[allow(deprecated)]
                    let get_home = c.command == MAV_CMD_GET_HOME_POSITION;
                    let asks_home = get_home || (request && s.request_message && c.param1 == 242.0);
                    if let Some((latitude, longitude, altitude)) = s.home.filter(|_| asks_home) {
                        out.push(Msg::HOME_POSITION(dialect::HOME_POSITION_DATA {
                            latitude,
                            longitude,
                            altitude,
                            ..Default::default()
                        }));
                    }
                }
                Msg::PARAM_REQUEST_LIST(_) => {
                    for i in 0..s.params.len() {
                        if !s.drop_params.contains(&(i as u16)) {
                            out.push(param_value(&s, i));
                        }
                    }
                }
                Msg::PARAM_REQUEST_READ(r) => {
                    if (r.param_index as usize) < s.params.len() {
                        out.push(param_value(&s, r.param_index as usize));
                    }
                }
                Msg::LOG_REQUEST_LIST(_) => {
                    for (id, size) in [(1u16, 1234u32), (2, s.log.len() as u32)] {
                        out.push(Msg::LOG_ENTRY(dialect::LOG_ENTRY_DATA {
                            time_utc: 1_700_000_000 + id as u32,
                            size,
                            id,
                            num_logs: 2,
                            last_log_num: 2,
                        }));
                    }
                }
                Msg::LOG_REQUEST_DATA(r) => {
                    let end = (r.ofs as u64 + r.count as u64).min(s.log.len() as u64) as u32;
                    let mut ofs = r.ofs;
                    while ofs < end {
                        if s.log_cut.is_some_and(|cut| ofs >= cut) {
                            break;
                        }
                        let count = (end - ofs).min(90);
                        if !s.log_drop_once.remove(&ofs) {
                            let mut data = [0u8; 90];
                            data[..count as usize]
                                .copy_from_slice(&s.log[ofs as usize..(ofs + count) as usize]);
                            out.push(Msg::LOG_DATA(dialect::LOG_DATA_DATA {
                                ofs,
                                id: r.id,
                                count: count as u8,
                                data,
                            }));
                        }
                        ofs += count;
                    }
                }
                Msg::SERIAL_CONTROL(c) if c.count > 0 => {
                    let mut text = b"echo: ".to_vec();
                    text.extend_from_slice(&c.data[..c.count as usize]);
                    let mut data = [0u8; 70];
                    let n = text.len().min(70);
                    data[..n].copy_from_slice(&text[..n]);
                    out.push(Msg::SERIAL_CONTROL(dialect::SERIAL_CONTROL_DATA {
                        device: dialect::SerialControlDev::SERIAL_CONTROL_DEV_SHELL,
                        flags: dialect::SerialControlFlag::SERIAL_CONTROL_FLAG_REPLY,
                        count: n as u8,
                        data,
                        ..Default::default()
                    }));
                }
                _ => {}
            }
            drop(s);
            for msg in out {
                send(&mut wire, &mut enc, &msg);
            }
        }
    }
}
