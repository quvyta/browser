//! What the engine's tests stand on: scratch folders, a local web server with fixed pages, a
//! limit on how many Chromiums run at once, and a started engine with bounded waits.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::engine::{Button, Engine, Event, Modifiers, Mouse, Options, StartError, TabId};

/// Every wait in these tests: generous, so a loaded machine does not fail them, and finite.
pub(crate) const PATIENCE: Duration = Duration::from_secs(20);

/// The Chromium the tests run.
pub(crate) const CHROMIUM: &str = "/usr/bin/chromium";

/// A folder of its own for one test, removed afterwards.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("qbrowser-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// A subfolder, made.
    pub(crate) fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    /// Options running the test Chromium with `<scratch>/home` and `<scratch>/temp`.
    pub(crate) fn options(&self) -> Options {
        Options {
            chromium: Some(PathBuf::from(CHROMIUM)),
            path_var: None,
            profile_home: self.0.join("home"),
            temp_root: self.0.join("temp"),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How many tests may run Chromium at the same time: each one takes a few hundred megabytes,
/// and `cargo test` runs as many tests at once as the machine has cores.
const CHROMIUM_SLOTS: usize = 3;

static SLOTS: Mutex<usize> = Mutex::new(0);
static FREED: Condvar = Condvar::new();

/// A test's permission to run Chromium, given back when dropped.
pub(crate) struct Slot;

impl Slot {
    /// Waits until fewer than [`CHROMIUM_SLOTS`] tests run Chromium.
    pub(crate) fn take() -> Self {
        let mut taken = lock(&SLOTS);
        while *taken >= CHROMIUM_SLOTS {
            taken = FREED.wait(taken).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *taken += 1;
        Slot
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        *lock(&SLOTS) -= 1;
        FREED.notify_one();
    }
}

/// A lock that a panicking test does not poison for the others.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The address of `path` on the tests' web server, started on first use.
pub(crate) fn page(path: &str) -> String {
    static PORT: OnceLock<u16> = OnceLock::new();
    let port = *PORT.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                thread::spawn(move || serve(&stream));
            }
        });
        port
    });
    format!("http://127.0.0.1:{port}{path}")
}

/// A big block that is easy to hit with the mouse.
const BIG: &str = "display:block;position:absolute;left:40px;top:40px;width:300px;height:100px;font-size:40px";

/// The body of the page at `path`, if there is one.
fn body(path: &str) -> Option<String> {
    let html = match path {
        "/links" => format!("<title>Links</title><a id=next href=/second style='{BIG}'>Next</a>"),
        "/second" => "<title>Second</title><p id=here>second</p>".to_owned(),
        "/third" => "<title>Third</title><p>third</p>".to_owned(),
        "/long" => {
            "<title>Long</title><div style='height:5000px;background:linear-gradient(red,blue)'>long</div>".to_owned()
        }
        "/form" => format!("<title>Form</title><input id=field style='{BIG}'>"),
        "/blank" => format!("<title>Blank</title><a id=out target=_blank href=/second style='{BIG}'>Out</a>"),
        "/opener" => {
            format!(
                "<title>Opener</title><button id=open onclick=\"window.open('/closer')\" style='{BIG}'>Open</button>"
            )
        }
        "/closer" => "<title>Closer</title><script>setTimeout(() => window.close(), 500)</script>".to_owned(),
        // The title changes only after the page has loaded, so on a busy machine too the change
        // is heard after the load ends.
        "/title" => "<title>Before</title><script>addEventListener('load', () => setTimeout(() => document.title = 'After', 300))</script>"
            .to_owned(),
        _ => return None,
    };
    Some(format!("<!doctype html><html><body style='margin:0'>{html}</body></html>"))
}

fn serve(stream: &TcpStream) {
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    // The rest of the request head is read and ignored.
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
        line.clear();
    }
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    if path == "/slow" {
        serve_slowly(stream);
        return;
    }
    let response = match body(path) {
        Some(body) => format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
        None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
    };
    let mut out = stream;
    let _ = out.write_all(response.as_bytes());
}

/// The page at `/slow`: its start comes at once and its end only after twice [`PATIENCE`], so it
/// is loading for longer than any test waits, unless the browser stops it.
fn serve_slowly(stream: &TcpStream) {
    let mut out = stream;
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n";
    let start = "<!doctype html><html><body style='margin:0'><title>Slow</title><p>coming";
    if out.write_all(head.as_bytes()).and_then(|()| out.write_all(start.as_bytes())).and_then(|()| out.flush()).is_err()
    {
        return;
    }
    thread::sleep(PATIENCE * 2);
    let _ = out.write_all(b"</p></body></html>");
}

/// A started engine, the events it sent and bounded ways of waiting for them.
pub(crate) struct Browser {
    pub(crate) engine: Engine,
    events: Receiver<Event>,
}

impl Browser {
    pub(crate) fn start(options: &Options) -> Self {
        match Browser::try_start(options) {
            Ok(browser) => browser,
            Err(error) => panic!("the engine did not start: {error}"),
        }
    }

    pub(crate) fn try_start(options: &Options) -> Result<Self, StartError> {
        let (engine, events) = Engine::start(options)?;
        Ok(Self { engine, events })
    }

    /// Waits for the first event `pick` takes, skipping the others.
    pub(crate) fn wait<T>(&self, what: &str, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(event) => {
                    if let Some(found) = pick(&event) {
                        return found;
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("no {what} within {PATIENCE:?}"),
                Err(RecvTimeoutError::Disconnected) => panic!("the engine stopped before {what}"),
            }
        }
    }

    /// Opens a tab on `url`, shows it at 800 × 600 and waits until it is there.
    pub(crate) fn open(&self, url: &str) -> TabId {
        self.engine.open_tab(url);
        let tab = self.wait("the new tab", |event| match event {
            Event::TabOpened { tab, opener: None, .. } => Some(tab.clone()),
            _ => None,
        });
        self.engine.set_viewport(&tab, 800, 600);
        self.engine.show(&tab);
        self.arrive(&tab, url);
        tab
    }

    /// Waits until the tab reports being at `url`.
    pub(crate) fn arrive(&self, at: &TabId, url: &str) -> (bool, bool) {
        self.wait(&format!("arrival at {url}"), |event| match event {
            Event::Navigated { tab, url: now, can_back, can_forward } if tab == at && now == url => {
                Some((*can_back, *can_forward))
            }
            _ => None,
        })
    }

    /// The value of `expression` in the tab.
    pub(crate) fn eval(&self, tab: &TabId, expression: &str) -> Value {
        match self.engine.evaluate(tab, expression, PATIENCE) {
            Ok(value) => value,
            Err(error) => panic!("`{expression}` failed: {error}"),
        }
    }

    /// Asks the tab `expression` every little while until it is true.
    pub(crate) fn until(&self, tab: &TabId, expression: &str) {
        let deadline = Instant::now() + PATIENCE;
        while self.eval(tab, expression) != Value::Bool(true) {
            assert!(Instant::now() < deadline, "`{expression}` did not become true within {PATIENCE:?}");
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// The middle of the element `selector` picks, in CSS pixels of the viewport.
    pub(crate) fn middle_of(&self, tab: &TabId, selector: &str) -> (f64, f64) {
        let rect = self.eval(
            tab,
            &format!("(() => {{ const r = document.querySelector('{selector}').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; }})()"),
        );
        (rect[0].as_f64().unwrap(), rect[1].as_f64().unwrap())
    }

    /// Clicks the left button at (`x`, `y`), as a person's hand does: move there, press, release.
    pub(crate) fn click(&self, tab: &TabId, (x, y): (f64, f64)) {
        let plain = Modifiers::default();
        self.engine.mouse(tab, Mouse::Moved { held: None }, x, y, plain);
        self.engine.mouse(tab, Mouse::Pressed { button: Button::Left, clicks: 1 }, x, y, plain);
        self.engine.mouse(tab, Mouse::Released { button: Button::Left, clicks: 1 }, x, y, plain);
    }
}

/// The width and height a baseline or progressive JPEG declares in its frame header.
pub(crate) fn jpeg_size(jpeg: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    while at + 9 < jpeg.len() {
        if jpeg[at] != 0xFF {
            return None;
        }
        let marker = jpeg[at + 1];
        let length = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        if matches!(marker, 0xC0..=0xC2) {
            let height = u16::from_be_bytes([jpeg[at + 5], jpeg[at + 6]]);
            let width = u16::from_be_bytes([jpeg[at + 7], jpeg[at + 8]]);
            return Some((u32::from(width), u32::from(height)));
        }
        at += 2 + length;
    }
    None
}

/// Every running process whose command line mentions `text`.
pub(crate) fn processes_mentioning(text: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(text))
        })
        .collect()
}

/// Waits until no running process mentions `text`, and says which still do if some never leave.
pub(crate) fn wait_until_none_mention(text: &str) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let left = processes_mentioning(text);
        if left.is_empty() {
            return;
        }
        assert!(Instant::now() < deadline, "processes still run on {text}: {left:?}");
        thread::sleep(Duration::from_millis(50));
    }
}
