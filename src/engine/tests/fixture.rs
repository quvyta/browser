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
///
/// Sixty seconds, because on a machine that builds other projects at the same time a page often
/// takes longer than twenty to arrive: eight gate runs on a loaded machine dropped five to twelve
/// Chromium tests each with "no arrival within 20s", and nothing about the browser was wrong. The
/// wait still ends, so a test that waits for something that never comes still fails. This is the
/// tests' patience only; how long the product waits for Chromium to start is its own.
pub(crate) const PATIENCE: Duration = Duration::from_secs(60);

/// The Chromium the tests run: `QBROW_TEST_CHROMIUM` when the build had it, `/usr/bin/chromium`
/// otherwise (see `build.rs`, which also decides whether the tests that need it run at all).
pub(crate) const CHROMIUM: &str = match option_env!("QBROW_TEST_CHROMIUM") {
    Some(path) => path,
    None => "/usr/bin/chromium",
};

/// The switches that keep a test Chromium off the internet and out of the person's own folders:
/// every request goes to a proxy on a port nobody listens on, so a page on a real site fails at
/// once and Chromium's own background requests (safe browsing lists, component updates) go
/// nowhere; the local test server is reached directly, as Chromium never sends loopback
/// addresses through a proxy. No crash reports are written, which would land in the person's own
/// `~/.config/chromium` whatever the profile.
pub(crate) fn isolated() -> Vec<std::ffi::OsString> {
    [
        "--proxy-server=http://127.0.0.1:9",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-breakpad",
    ]
    .into_iter()
    .map(Into::into)
    .collect()
}

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
            extra_arguments: isolated(),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How many tests may run Chromium at the same time when `QBROW_TEST_CHROMIUMS` does not say:
/// one. Each Chromium takes a few hundred megabytes and several processes, `cargo test` runs as
/// many tests at once as the machine has cores, and the machines these tests run on build other
/// projects at the same time; three at once ran them out of memory.
const CHROMIUM_SLOTS: usize = 1;

/// Below this much available memory a test waits before it starts a Chromium, so the tests do
/// not push a busy machine into swapping or into its out-of-memory killer.
const MEMORY_FLOOR_KB: u64 = 3 * 1024 * 1024;

/// How long a test waits for memory to free up before it starts its Chromium anyway: generous,
/// and finite, so a machine that never frees that much still runs the tests.
const MEMORY_WAIT: Duration = Duration::from_secs(600);

/// The slots in use now, and how many there are.
static SLOTS: Mutex<usize> = Mutex::new(0);
static FREED: Condvar = Condvar::new();

/// How many tests may run Chromium at once: `QBROW_TEST_CHROMIUMS` when it is a positive number.
fn chromium_slots() -> usize {
    std::env::var("QBROW_TEST_CHROMIUMS").ok().and_then(|n| n.parse().ok()).filter(|&n| n > 0).unwrap_or(CHROMIUM_SLOTS)
}

/// The memory the system says it can give without swapping, from `/proc/meminfo`; `None` where it
/// does not say.
fn available_kb() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = meminfo.lines().find(|line| line.starts_with("MemAvailable:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Waits, at most [`MEMORY_WAIT`], until the system has [`MEMORY_FLOOR_KB`] to spare.
fn wait_for_memory() {
    let deadline = Instant::now() + MEMORY_WAIT;
    while available_kb().is_some_and(|kb| kb < MEMORY_FLOOR_KB) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(500));
    }
}

/// A test's permission to run Chromium, given back when dropped.
///
/// Every test that takes one also carries `#[cfg_attr(not(chromium), ignore = "needs Chromium")]`,
/// so a machine without Chromium lists it as ignored instead of failing it after a long wait;
/// [`every_test_that_runs_chromium_is_ignored_without_it`] holds them to that.
pub(crate) struct Slot(usize);

impl Slot {
    /// Waits until fewer than [`chromium_slots`] tests run Chromium, then until the machine has
    /// memory to spare for one more.
    pub(crate) fn take() -> Self {
        Self::take_many(1)
    }

    /// Permission for a test that runs `chromiums` Chromiums at once. It waits until they all
    /// fit, or, when there are fewer slots than that, until it is the only test running one: a
    /// test that took its slots one by one would wait for itself forever.
    pub(crate) fn take_many(chromiums: usize) -> Self {
        let slots = chromium_slots();
        let mut taken = lock(&SLOTS);
        while *taken > 0 && *taken + chromiums > slots {
            taken = FREED.wait(taken).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *taken += chromiums;
        drop(taken);
        wait_for_memory();
        Slot(chromiums)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        *lock(&SLOTS) -= self.0;
        FREED.notify_all();
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

/// An article with the furniture of a page around it and inside it: a navigation bar, a sidebar
/// and a footer, both inside the article and outside it, whose words belong to the page but not to
/// what it has to say, and in the middle every kind of text a reading has to carry.
///
/// The page's own scripts count what they are asked for, so a test can see afterwards whether the
/// reading was read where the page cannot see it, or whether the page's own world was used.
const ARTICLE: &str = r#"<script>
window.__seen = 0;
for (const [proto, name] of [
  [Element.prototype, 'querySelector'], [Element.prototype, 'querySelectorAll'],
  [Document.prototype, 'querySelector'], [Document.prototype, 'querySelectorAll'],
]) {
  const asked = proto[name];
  proto[name] = function () { window.__seen += 1; return asked.apply(this, arguments); };
}
</script>
<title>Reading in the terminal</title>
<nav>Sign in Register Support</nav>
<div>
<p>Related reading, the kind of thing a page collects beside its article: another writer's post
with a paragraph of its own, and another after it, and a third, so that this corner of the page
holds more words in its own paragraphs than the article the reader came for does. A page is often
like this, and a reading that counted words alone would hand the reader somebody else's post
instead of the one they asked for.</p>
<p>A second paragraph of the same related reading, which adds a little more of the same, so that
the total is plainly past the article's own two paragraphs and there is no doubt which part of the
page holds the most text once the words are counted.</p>
<p>And a third, to be sure of it.</p>
</div>
<article>
<nav>Contents Installation Configuration</nav>
<h1>The page's own words</h1>
<p>A browser draws a page as a picture, and in a terminal one cell is two pixels tall, so the
writing of a page is small enough that a person has to lean towards the screen to read it.
Reading mode takes the page's own words out of that picture and writes them with the terminal's
own letters, at whatever size the terminal draws them.</p>
<p>A second paragraph, with <strong>strong words</strong>, some <em>emphasised ones</em>, a piece
of <code>inline code</code> and a <a href='/second'>link whose address is not part of what it
says</a>.</p>
<ul><li>The first item<ul><li>and an item nested inside it</li><li>with a second nested item</li></ul></li><li>The second item</li></ul>
<blockquote>A line worth keeping for its own sake.</blockquote>
<pre>fn main() {
    let answer = 42;
    if answer &gt; 0 {
        print(answer);
    }
}</pre>
<aside>Newsletter one letter a month</aside>
<table><tr><th>Key</th><th>What it opens</th></tr><tr><td>reader</td><td>reading mode, the page's own text</td></tr></table>
<img src='/pixel.png' alt='A quiet harbour at dusk'>
<footer>Copyright nobody. All rights reserved.</footer>
</article>
<footer>Copyright nobody. All rights reserved.</footer>"#;

/// An article whose words are all ASCII, for the screen of a terminal whose font has nothing but
/// ASCII in it: a reading of this page must not bring a single other character onto that screen.
const NARROW: &str = r#"<title>Notes in plain words</title>
<article>
<h1>Notes in plain words</h1>
<p>A terminal that draws no picture at all can still write, and a page of writing read there is
readable where a picture of the same page is not. This article says so at some length, so that
there is a whole screen of words to scroll through and every kind of them to look at.</p>
<ul><li>The first note</li><li>The second note, with a <a href='/second'>link inside it</a></li></ul>
<pre>def draw(screen):
    return screen</pre>
<p>The last paragraph, so that the reading is taller than the area it is drawn in and the keys
and the wheel have something to scroll.</p>
</article>"#;

/// A form: a select with a disabled option and a group, a list that shows several rows itself, a
/// button and a line of words. The page writes what its own listeners hear where the tests read it,
/// and counts every call of the two functions a question about the pointer would use, so a question
/// asked where the page could see it would show.
const SELECT: &str = "<select id=fruit style='position:absolute;left:40px;top:40px;width:200px;height:40px;font-size:20px'>\
         <option value=apple>Apple</option><option value=banana selected>Banana</option>\
         <option value=cherry disabled>Cherry</option>\
         <optgroup label=Citrus><option value=lemon>Lemon</option><option value=lime>Lime</option></optgroup>\
         </select>\
         <select id=many multiple style='position:absolute;left:40px;top:300px;width:200px;height:100px;font-size:20px'>\
         <option>One</option><option>Two</option><option>Three</option></select>\
         <button id=go style='position:absolute;left:600px;top:300px;width:200px;height:60px'>go</button>\
         <p id=words style='position:absolute;left:400px;top:120px;margin:0;font-size:30px'>double click here</p>\
         <script>\
         window.seen = []; window.pressed = 0; window.went = 0; window.doubled = 0; window.spied = 0;\
         fruit.addEventListener('input', () => seen.push('input'));\
         fruit.addEventListener('change', e => seen.push(e.target.value));\
         fruit.addEventListener('mousedown', () => pressed++);\
         go.addEventListener('click', () => went++);\
         words.addEventListener('dblclick', () => doubled++);\
         const point = document.elementFromPoint.bind(document);\
         document.elementFromPoint = (x, y) => { spied++; return point(x, y); };\
         const closest = Element.prototype.closest;\
         Element.prototype.closest = function (s) { spied++; return closest.call(this, s); };\
         </script>";

/// The body of the page at `path`, if there is one.
fn body(path: &str) -> Option<String> {
    let html = match path {
        "/links" => format!("<title>Links</title><a id=next href=/second style='{BIG}'>Next</a>"),
        "/second" => "<title>Second</title><p id=here>second</p>".to_owned(),
        "/third" => "<title>Third</title><p>third</p>".to_owned(),
        "/long" => {
            "<title>Long</title><div style='height:5000px;background:linear-gradient(red,blue)'>long</div>".to_owned()
        }
        // Red to its edges, with a blue bar three cells of nine pixels wide down the right edge that
        // counts its clicks.
        "/edges" => "<title>Edges</title><style>html,body{margin:0;height:100%;background:#c80000}</style>\
                     <button id=edge style='position:fixed;right:0;top:0;width:27px;height:100%;border:0;padding:0;background:#0000c8'></button>\
                     <script>let went = 0; edge.addEventListener('click', () => went++);</script>"
            .to_owned(),
        "/form" => format!("<title>Form</title><input id=field style='{BIG}'>"),
        "/blank" => format!("<title>Blank</title><a id=out target=_blank href=/second style='{BIG}'>Out</a>"),
        "/opener" => {
            format!(
                "<title>Opener</title><button id=open onclick=\"window.open('/closer')\" style='{BIG}'>Open</button>"
            )
        }
        // Each button opens one of the page's own dialogs and writes the answer into the title,
        // so a test sees both that the dialog showed and what it answered.
        "/alert" => format!(
            "<title>Alert</title><button id=open onclick=\"alert('Hello from the page'); document.title = 'Alerted'\" style='{BIG}'>Alert</button>"
        ),
        "/confirm" => format!(
            "<title>Confirm</title><button id=open onclick=\"document.title = confirm('Go on?') ? 'Yes' : 'No'\" style='{BIG}'>Confirm</button>"
        ),
        "/prompt" => format!(
            "<title>Prompt</title><button id=open onclick=\"document.title = 'Answer ' + prompt('Your name?', 'Ada')\" style='{BIG}'>Prompt</button>"
        ),
        "/closer" => "<title>Closer</title><script>setTimeout(() => window.close(), 500)</script>".to_owned(),
        // The title changes only after the page has loaded, so on a busy machine too the change
        // is heard after the load ends.
        "/title" => "<title>Before</title><script>addEventListener('load', () => setTimeout(() => document.title = 'After', 300))</script>"
            .to_owned(),
        "/article" => ARTICLE.to_owned(),
        "/narrow" => NARROW.to_owned(),
        "/select" => SELECT.to_owned(),
        "/sparse" => "<title>Sparse</title><p>Nothing here but a word.</p>".to_owned(),
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
        // A page between two documents has no context to run in for a moment; that is a "not yet",
        // not a failure, while the tab is on its way to where the expression becomes true.
        let holds = || match self.engine.evaluate(tab, expression, PATIENCE) {
            Ok(value) => value == Value::Bool(true),
            Err(error) if error.contains("execution context") => false,
            Err(error) => panic!("`{expression}` failed: {error}"),
        };
        while !holds() {
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

/// Without Chromium the tests that drive it are ignored, each with the reason "needs Chromium";
/// this one stands in the list for all of them, so the run says plainly what it did not check.
#[cfg(not(chromium))]
#[test]
#[ignore = "no Chromium at QBROW_TEST_CHROMIUM (default /usr/bin/chromium): every test that drives a real Chromium is ignored"]
fn chromium_is_missing_so_the_tests_that_drive_it_did_not_run() {}

/// A test that runs Chromium without the mark would fail on a machine without it after waiting
/// out its patience, instead of being listed as ignored.
#[test]
fn every_test_that_runs_chromium_is_ignored_without_it() {
    const MARK: &str = "#[cfg_attr(not(chromium), ignore = \"needs Chromium\")]";
    fn visit(folder: &Path, unmarked: &mut Vec<String>) {
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, unmarked);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                // This file names what it looks for in the text of the check below.
                if path.ends_with("fixture.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                let lines: Vec<&str> = text.lines().collect();
                let starts: Vec<usize> = (0..lines.len()).filter(|&at| lines[at].trim() == "#[test]").collect();
                for (n, &start) in starts.iter().enumerate() {
                    let body = &lines[start..starts.get(n + 1).copied().unwrap_or(lines.len())];
                    let runs_chromium = body.iter().any(|line| {
                        ["Slot::take", "(CHROMIUM", "::CHROMIUM", "Browser::start("]
                            .iter()
                            .any(|needle| line.contains(needle))
                    });
                    if runs_chromium && !body.iter().take(3).any(|line| line.trim() == MARK) {
                        unmarked.push(format!("{}:{}", path.display(), start + 1));
                    }
                }
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut unmarked = Vec::new();
    visit(&root.join("src"), &mut unmarked);
    visit(&root.join("tests"), &mut unmarked);
    assert!(unmarked.is_empty(), "tests that run Chromium without the mark {MARK}:\n{}", unmarked.join("\n"));
}
