//! `AgentClient`: one JSON-lines connection to a WSL agent, reconnected on demand.
//!
//! Requests carry ids; each has a deadline and can be cancelled. A reply that arrives after its
//! request timed out or was cancelled is dropped. Events fan out to bounded, coalescing
//! [`Subscription`]s.
use super::{AgentEvent, Hello};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    io::{self, BufReader, Read, Write},
    process::Child,
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

/// The agent process seen from the client: whatever runs it (wsl.exe, or a test thread).
pub(crate) trait Process: Send {
    fn kill(&mut self);
    /// The exit code once the process has ended, waiting at most `wait` for it.
    fn exit_code(&mut self, wait: Duration) -> Option<i32>;
}
impl Process for Child {
    fn kill(&mut self) {
        let _ = Child::kill(self);
        let _ = self.wait();
    }
    fn exit_code(&mut self, wait: Duration) -> Option<i32> {
        let deadline = Instant::now() + wait;
        loop {
            match self.try_wait() {
                Ok(Some(status)) => return status.code(),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                _ => return None,
            }
        }
    }
}

/// A started agent: its stdout, stdin, optional stderr and process handle.
pub(crate) struct Spawned {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub errors: Option<Box<dyn Read + Send>>,
    pub process: Box<dyn Process>,
}
/// Starts an agent. Called lazily, and again after the agent exits.
pub(crate) type Connector = Arc<dyn Fn() -> io::Result<Spawned> + Send + Sync>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AgentError {
    /// The agent could not be started (no WSL, no python3, preview mode…).
    Unavailable(String),
    /// The agent exited before replying.
    Disconnected,
    Timeout,
    /// The agent replied with an error message.
    Remote(String),
}
impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(e) => write!(f, "The Linux agent could not start: {e}"),
            Self::Disconnected => f.write_str("The Linux agent stopped before replying."),
            Self::Timeout => f.write_str("The Linux agent did not reply in time."),
            Self::Remote(e) => f.write_str(e),
        }
    }
}
pub(crate) type Reply = Result<Value, AgentError>;

struct Waiter {
    tx: Sender<Reply>,
    progress: Option<Sender<String>>,
}
struct Conn {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Mutex<HashMap<u64, Waiter>>,
    alive: AtomicBool,
    /// Set by [`AgentClient::close`]: the exit that follows is expected.
    closing: AtomicBool,
    process: Mutex<Box<dyn Process>>,
    /// Set once the reader published `Disconnected`: a reconnect waits for it, so subscribers
    /// always see the old connection end before the new one starts.
    finished: (Mutex<bool>, Condvar),
}
impl Conn {
    fn wait_finished(&self, limit: Duration) {
        let done = self.finished.0.lock().unwrap();
        let _ = self
            .finished
            .1
            .wait_timeout_while(done, limit, |done| !*done)
            .unwrap();
    }
    fn write(&self, value: &Value) -> io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writeln!(writer, "{value}")?;
        writer.flush()
    }
    fn forget(&self, id: u64) -> bool {
        self.pending.lock().unwrap().remove(&id).is_some()
    }
}

/// One request in flight. Dropping it forgets the request: a late reply is discarded.
pub(crate) struct Ticket {
    id: u64,
    deadline: Instant,
    rx: Receiver<Reply>,
    conn: Option<Arc<Conn>>,
    done: bool,
}
impl Ticket {
    fn failed(error: AgentError) -> Self {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(Err(error));
        Self {
            id: 0,
            deadline: Instant::now(),
            rx,
            conn: None,
            done: false,
        }
    }
    #[cfg(test)]
    pub(crate) fn id(&self) -> u64 {
        self.id
    }
    fn expire(&mut self) -> Reply {
        self.done = true;
        if let Some(conn) = &self.conn
            && conn.forget(self.id)
        {
            let _ = conn.write(&json!({"method":"cancel","params":{"id":self.id}}));
        }
        // A reply may have raced the deadline; prefer it.
        self.rx.try_recv().unwrap_or(Err(AgentError::Timeout))
    }
    /// The reply if it has arrived, `Timeout` once the deadline passes, else `None`.
    pub(crate) fn try_take(&mut self) -> Option<Reply> {
        if self.done {
            return None;
        }
        match self.rx.try_recv() {
            Ok(reply) => {
                self.done = true;
                Some(reply)
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.done = true;
                Some(Err(AgentError::Disconnected))
            }
            Err(mpsc::TryRecvError::Empty) if Instant::now() >= self.deadline => {
                Some(self.expire())
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
    /// Blocks until the reply arrives or the deadline passes.
    pub(crate) fn wait(mut self) -> Reply {
        let left = self.deadline.saturating_duration_since(Instant::now());
        match self.rx.recv_timeout(left) {
            Ok(reply) => {
                self.done = true;
                reply
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.done = true;
                Err(AgentError::Disconnected)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => self.expire(),
        }
    }
    /// Gives up on the request and asks the agent to stop working on it.
    pub(crate) fn cancel(mut self) {
        self.done = true;
        if let Some(conn) = &self.conn
            && conn.forget(self.id)
        {
            let _ = conn.write(&json!({"method":"cancel","params":{"id":self.id}}));
        }
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        if !self.done
            && let Some(conn) = &self.conn
        {
            conn.forget(self.id);
        }
    }
}

/// Queue depth per subscription for output lines; other events get four times as much room.
const SUBSCRIPTION_CAPACITY: usize = 2048;
struct Queue {
    events: VecDeque<AgentEvent>,
    dropped: u64,
}
type Shared = (Mutex<Queue>, Condvar);
/// Which events a subscription keeps; the rest never take room in its queue.
pub(crate) type Filter = fn(&AgentEvent) -> bool;
/// A bounded view of the agent's events. Router updates coalesce to the latest; output lines
/// beyond the bound are counted and reported as [`AgentEvent::Dropped`].
pub(crate) struct Subscription {
    shared: Arc<Shared>,
}
impl Subscription {
    /// Up to `max` queued events, oldest first.
    pub(crate) fn drain(&self, max: usize) -> Vec<AgentEvent> {
        let mut queue = self.shared.0.lock().unwrap();
        let mut events = Vec::new();
        if queue.dropped > 0 {
            events.push(AgentEvent::Dropped(std::mem::take(&mut queue.dropped)));
        }
        while events.len() < max {
            let Some(event) = queue.events.pop_front() else {
                break;
            };
            events.push(event);
        }
        events
    }
    /// Waits up to `timeout` for at least one event, then drains up to `max`.
    pub(crate) fn wait(&self, timeout: Duration, max: usize) -> Vec<AgentEvent> {
        let queue = self.shared.0.lock().unwrap();
        let _ = self
            .shared
            .1
            .wait_timeout_while(queue, timeout, |q| q.events.is_empty() && q.dropped == 0)
            .unwrap();
        self.drain(max)
    }
}
#[derive(Default)]
struct Hub {
    subscribers: Mutex<Vec<(Weak<Shared>, Filter)>>,
}
impl Hub {
    fn subscribe(&self, filter: Filter) -> Subscription {
        let shared = Arc::new((
            Mutex::new(Queue {
                events: VecDeque::new(),
                dropped: 0,
            }),
            Condvar::new(),
        ));
        self.subscribers
            .lock()
            .unwrap()
            .push((Arc::downgrade(&shared), filter));
        Subscription { shared }
    }
    fn publish(&self, event: AgentEvent) {
        let mut subscribers = self.subscribers.lock().unwrap();
        subscribers.retain(|(weak, filter)| {
            let Some(shared) = weak.upgrade() else {
                return false;
            };
            if !filter(&event) {
                return true;
            }
            let mut queue = shared.0.lock().unwrap();
            let len = queue.events.len();
            match &event {
                AgentEvent::Router(_) => {
                    queue.events.retain(|e| !matches!(e, AgentEvent::Router(_)));
                    queue.events.push_back(event.clone());
                }
                AgentEvent::Output { .. } if len >= SUBSCRIPTION_CAPACITY => queue.dropped += 1,
                _ if len >= SUBSCRIPTION_CAPACITY * 4 => queue.dropped += 1,
                _ => queue.events.push_back(event.clone()),
            }
            drop(queue);
            shared.1.notify_all();
            true
        });
    }
}

/// A client for one distribution's agent. Cheap to share (`Arc`); every method is thread-safe.
pub(crate) struct AgentClient {
    pub distro: String,
    connector: Connector,
    conn: Mutex<Option<Arc<Conn>>>,
    hub: Arc<Hub>,
    next_id: AtomicU64,
    hello: Arc<Mutex<Option<Hello>>>,
}
impl AgentClient {
    pub(crate) fn new(distro: impl Into<String>, connector: Connector) -> Arc<Self> {
        Arc::new(Self {
            distro: distro.into(),
            connector,
            conn: Mutex::new(None),
            hub: Arc::default(),
            next_id: AtomicU64::new(1),
            hello: Arc::default(),
        })
    }
    /// Every event, from now on. The app's views use [`Self::subscribe_filtered`].
    #[cfg(test)]
    pub(crate) fn subscribe(&self) -> Subscription {
        self.hub.subscribe(|_| true)
    }
    /// Only the events `keep` accepts (e.g. no process output for a view that ignores it, so
    /// output bursts neither fill its queue nor count as dropped).
    pub(crate) fn subscribe_filtered(&self, keep: Filter) -> Subscription {
        self.hub.subscribe(keep)
    }
    pub(crate) fn connected(&self) -> bool {
        self.conn
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|c| c.alive.load(Ordering::SeqCst))
    }
    /// The last `hello` reply of the current connection.
    pub(crate) fn hello(&self) -> Option<Hello> {
        self.hello.lock().unwrap().clone()
    }
    /// Starts the agent if it is not running. Publishes `Connected`, then `Hello` once it replies.
    pub(crate) fn connect(&self) -> Result<(), AgentError> {
        self.connection().map(|_| ())
    }
    fn connection(&self) -> Result<Arc<Conn>, AgentError> {
        let mut guard = self.conn.lock().unwrap();
        if let Some(conn) = guard.as_ref() {
            if conn.alive.load(Ordering::SeqCst) {
                return Ok(conn.clone());
            }
            // The reader waits up to 2 s for the exit code before it publishes `Disconnected`.
            conn.wait_finished(Duration::from_secs(3));
        }
        let spawned = (self.connector)().map_err(|e| AgentError::Unavailable(e.to_string()))?;
        let conn = Arc::new(Conn {
            writer: Mutex::new(spawned.writer),
            pending: Mutex::new(HashMap::new()),
            alive: AtomicBool::new(true),
            closing: AtomicBool::new(false),
            process: Mutex::new(spawned.process),
            finished: (Mutex::new(false), Condvar::new()),
        });
        *self.hello.lock().unwrap() = None;
        let (reader, hub) = (conn.clone(), self.hub.clone());
        thread::spawn(move || read_loop(spawned.reader, reader, hub));
        if let Some(errors) = spawned.errors {
            let hub = self.hub.clone();
            thread::spawn(move || {
                for line in
                    crate::process_output::BoundedLines::new(BufReader::new(errors)).flatten()
                {
                    hub.publish(AgentEvent::Log {
                        level: "stderr".into(),
                        message: line,
                    });
                }
            });
        }
        *guard = Some(conn.clone());
        drop(guard);
        self.hub.publish(AgentEvent::Connected);
        let ticket = self.send(&conn, "hello", json!({}), Duration::from_secs(60), None);
        let (hub, hello) = (self.hub.clone(), self.hello.clone());
        thread::spawn(move || match ticket.wait() {
            Ok(value) => {
                let parsed: Hello = serde_json::from_value(value).unwrap_or_default();
                *hello.lock().unwrap() = Some(parsed.clone());
                hub.publish(AgentEvent::Hello(parsed));
            }
            Err(e) => hub.publish(AgentEvent::Log {
                level: "error".into(),
                message: format!("The Linux agent did not answer hello: {e}"),
            }),
        });
        Ok(conn)
    }
    fn send(
        &self,
        conn: &Arc<Conn>,
        method: &str,
        params: Value,
        timeout: Duration,
        progress: Option<Sender<String>>,
    ) -> Ticket {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        conn.pending
            .lock()
            .unwrap()
            .insert(id, Waiter { tx, progress });
        let ticket = Ticket {
            id,
            deadline: Instant::now() + timeout,
            rx,
            conn: Some(conn.clone()),
            done: false,
        };
        let request = json!({"id":id,"method":method,"params":params});
        if conn.write(&request).is_err()
            && let Some(waiter) = conn.pending.lock().unwrap().remove(&id)
        {
            let _ = waiter.tx.send(Err(AgentError::Disconnected));
        }
        ticket
    }
    /// Sends a request without waiting; the ticket delivers the reply.
    pub(crate) fn request(&self, method: &str, params: Value, timeout: Duration) -> Ticket {
        self.request_with_progress(method, params, timeout, None)
    }
    /// Like [`Self::request`]; `progress` receives the agent's `progress` events for it.
    pub(crate) fn request_with_progress(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        progress: Option<Sender<String>>,
    ) -> Ticket {
        match self.connection() {
            Ok(conn) => self.send(&conn, method, params, timeout, progress),
            Err(e) => Ticket::failed(e),
        }
    }
    /// Sends a request and blocks for its reply.
    pub(crate) fn call(&self, method: &str, params: Value, timeout: Duration) -> Reply {
        self.request(method, params, timeout).wait()
    }
    /// Closes stdin (the agent leaves owned processes running) and waits up to `wait` for exit.
    pub(crate) fn close(&self, wait: Duration) {
        let Some(conn) = self.conn.lock().unwrap().take() else {
            return;
        };
        conn.closing.store(true, Ordering::SeqCst);
        let _ = conn.write(&json!({"method":"shutdown"}));
        {
            let mut process = conn.process.lock().unwrap();
            if process.exit_code(wait).is_none() && conn.alive.load(Ordering::SeqCst) {
                process.kill();
            }
        }
        // `Disconnected { expected: true }` is out before a later request starts a new agent.
        conn.wait_finished(Duration::from_secs(3));
    }
}
impl Drop for AgentClient {
    fn drop(&mut self) {
        self.close(Duration::from_secs(5));
    }
}

fn read_loop(reader: Box<dyn Read + Send>, conn: Arc<Conn>, hub: Arc<Hub>) {
    // Replies such as rec.list or data.library can be large; events are bounded by the agent.
    for line in crate::process_output::BoundedLines::with_limit(BufReader::new(reader), 16_000_000)
        .flatten()
    {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            hub.publish(AgentEvent::Log {
                level: "stdout".into(),
                message: line.chars().take(3000).collect(),
            });
            continue;
        };
        if let Some(event) = value.get("event").and_then(Value::as_str) {
            if event == "progress" {
                let id = value["id"].as_u64().unwrap_or(0);
                if let Some(progress) = conn
                    .pending
                    .lock()
                    .unwrap()
                    .get(&id)
                    .and_then(|w| w.progress.as_ref())
                {
                    let _ = progress.send(value["message"].as_str().unwrap_or("").to_owned());
                }
                continue;
            }
            hub.publish(AgentEvent::parse(event, &value));
        } else if let Some(id) = value.get("id").and_then(Value::as_u64) {
            let reply = match value.get("error") {
                Some(error) => Err(AgentError::Remote(
                    error["message"]
                        .as_str()
                        .unwrap_or("The Linux agent reported an error.")
                        .to_owned(),
                )),
                None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
            };
            // A missing waiter means the request timed out or was cancelled: drop the reply.
            if let Some(waiter) = conn.pending.lock().unwrap().remove(&id) {
                let _ = waiter.tx.send(reply);
            }
        }
    }
    conn.alive.store(false, Ordering::SeqCst);
    for (_, waiter) in conn.pending.lock().unwrap().drain() {
        let _ = waiter.tx.send(Err(AgentError::Disconnected));
    }
    let code = conn
        .process
        .lock()
        .unwrap()
        .exit_code(Duration::from_secs(2));
    hub.publish(AgentEvent::Disconnected {
        code,
        expected: conn.closing.load(Ordering::SeqCst),
    });
    *conn.finished.0.lock().unwrap() = true;
    conn.finished.1.notify_all();
}
