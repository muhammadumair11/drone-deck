//! A fake agent for tests: speaks the JSON-lines protocol on a thread over in-memory pipes.
use super::{Connector, Spawned, client::Process};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, PipeWriter, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// The fake's stdout. `close` simulates the agent exiting.
#[derive(Clone)]
pub(crate) struct Emit(Arc<Mutex<Option<PipeWriter>>>);
impl Emit {
    pub(crate) fn send(&self, value: Value) {
        if let Some(writer) = self.0.lock().unwrap().as_mut() {
            let _ = writeln!(writer, "{value}");
        }
    }
    pub(crate) fn raw(&self, line: &str) {
        if let Some(writer) = self.0.lock().unwrap().as_mut() {
            let _ = writeln!(writer, "{line}");
        }
    }
    pub(crate) fn reply(&self, request: &Value, result: Value) {
        self.send(json!({"id":request["id"],"result":result}));
    }
    pub(crate) fn error(&self, request: &Value, message: &str) {
        self.send(json!({"id":request["id"],"error":{"message":message}}));
    }
    pub(crate) fn close(&self) {
        self.0.lock().unwrap().take();
    }
    fn closed(&self) -> bool {
        self.0.lock().unwrap().is_none()
    }
}

struct FakeProcess(Emit);
impl Process for FakeProcess {
    fn kill(&mut self) {
        self.0.close();
    }
    fn exit_code(&mut self, wait: Duration) -> Option<i32> {
        let deadline = Instant::now() + wait;
        while !self.0.closed() {
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(5));
        }
        Some(0)
    }
}

/// Every request each fake agent received (cancel notifications included), and spawn count.
#[derive(Clone, Default)]
pub(crate) struct Seen {
    pub requests: Arc<Mutex<Vec<Value>>>,
    pub spawns: Arc<AtomicUsize>,
}
impl Seen {
    pub(crate) fn methods(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r["method"].as_str().unwrap_or("").to_owned())
            .collect()
    }
    pub(crate) fn spawns(&self) -> usize {
        self.spawns.load(Ordering::SeqCst)
    }
}

pub(crate) type Handler = Arc<dyn Fn(&Value, &Emit) + Send + Sync>;

/// A connector whose agents answer `hello` themselves and pass everything else to `handler`.
/// `shutdown` and end of input end the fake.
pub(crate) fn connector(handler: Handler) -> (Connector, Seen) {
    let seen = Seen::default();
    let record = seen.clone();
    let connector: Connector = Arc::new(move || {
        record.spawns.fetch_add(1, Ordering::SeqCst);
        let (request_reader, request_writer) = std::io::pipe()?;
        let (reply_reader, reply_writer) = std::io::pipe()?;
        let emit = Emit(Arc::new(Mutex::new(Some(reply_writer))));
        let (handler, record, out) = (handler.clone(), record.clone(), emit.clone());
        thread::spawn(move || {
            for line in BufReader::new(request_reader).lines().map_while(Result::ok) {
                let Ok(request) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                record.requests.lock().unwrap().push(request.clone());
                match request["method"].as_str() {
                    Some("shutdown") => break,
                    Some("hello") => out.reply(
                        &request,
                        json!({"version":1,"distro":"Fake","user":"pilot","home":"/home/pilot",
                            "automount_root":"/mnt/","python":"3.12.3"}),
                    ),
                    _ => handler(&request, &out),
                }
            }
            out.close();
        });
        Ok(Spawned {
            reader: Box::new(reply_reader),
            writer: Box::new(request_writer),
            errors: None,
            process: Box::new(FakeProcess(emit)),
        })
    });
    (connector, seen)
}

/// Polls `f` until it returns `Some` or five seconds pass.
pub(crate) fn eventually<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = f() {
            return value;
        }
        assert!(Instant::now() < deadline, "condition not met within 5 s");
        thread::sleep(Duration::from_millis(10));
    }
}
