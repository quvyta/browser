//! The real `qbrow` in a real pseudo-terminal, turned by the wheel the way a terminal reports it.
//!
//! The screen tests hand the framework's harness mouse events directly; this one writes the SGR
//! mouse reports a terminal sends for a wheel turn (`ESC [ < 65 ; column ; row M`) into the
//! terminal `qbrow` runs in, and reads what `qbrow` draws back. `script` from util-linux makes
//! the pseudo-terminal, so no terminal library and no unsafe code is needed.
//!
//! Nothing reaches the desktop or the network beyond this machine: the display, the session bus
//! and the runtime folder are not passed on, home and every XDG folder are a scratch folder, the
//! programs that open things on a desktop are fakes that only write down that they were called,
//! the update notice is off, and the page comes from a server on 127.0.0.1.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The Chromium the tests run, as in the screen tests.
const CHROMIUM: &str = "/usr/bin/chromium";

/// The longest any step waits: a loaded machine is slow, never stuck.
const PATIENCE: Duration = Duration::from_secs(30);

/// A page far taller than the screen, whose colour changes all the way down.
const LONG: &str = "<!doctype html><html><body style='margin:0'><title>Long</title>\
    <div style='height:5000px;background:linear-gradient(red,blue)'>long</div></body></html>";

/// The upper half block every cell of the page is drawn with in half-block mode.
const HALF: &str = "\u{2580}";

/// A folder removed when the test ends, however it ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
        let path = std::env::temp_dir().join(format!("qbrow-wheel-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Serves [`LONG`] for every request on 127.0.0.1 and returns its address.
fn serve_long_page() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/long", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                LONG.len()
            );
            let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(LONG.as_bytes()));
        }
    });
    address
}

/// Writes an executable shell script.
fn program(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// `qbrow` running on `address` in a 100 × 32 pseudo-terminal, with what it draws gathered as it
/// comes.
struct Running {
    child: Child,
    input: Arc<Mutex<ChildStdin>>,
    output: Arc<Mutex<Vec<u8>>>,
}

impl Running {
    fn start(scratch: &Scratch, address: &str) -> Self {
        let bin = scratch.dir("bin");
        let opened = scratch.0.join("opened");
        for name in ["xdg-open", "gio", "sensible-browser", "www-browser"] {
            program(&bin.join(name), &format!("echo \"$0 $*\" >> '{}'", opened.display()));
        }
        std::os::unix::fs::symlink(CHROMIUM, bin.join("chromium")).unwrap();
        let config = scratch.dir("config");
        std::fs::create_dir_all(config.join("quvyta")).unwrap();
        std::fs::write(config.join("quvyta/quvyta.conf"), "language = \"en\"\nupdate-notice = false\n").unwrap();
        let qbrow = env!("CARGO_BIN_EXE_qbrow");
        let inner = format!("stty rows 32 cols 100 && exec '{qbrow}' '{address}'");
        let mut child = Command::new("/usr/bin/script")
            .args(["--quiet", "--flush", "--return", "--command", &inner, "/dev/null"])
            .env_clear()
            .env("HOME", scratch.dir("home"))
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_DATA_HOME", scratch.dir("data"))
            .env("XDG_STATE_HOME", scratch.dir("state"))
            .env("XDG_CACHE_HOME", scratch.dir("cache"))
            .env("PATH", format!("{}:/usr/bin", bin.display()))
            .env("BROWSER", "true")
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("LANG", "C.UTF-8")
            .env("QUVYTA_GRAPHICS", "halfblock")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = Arc::new(Mutex::new(child.stdin.take().unwrap()));
        let output = Arc::new(Mutex::new(Vec::new()));
        let mut stdout = child.stdout.take().unwrap();
        {
            let (input, output) = (Arc::clone(&input), Arc::clone(&output));
            std::thread::spawn(move || {
                let mut chunk = [0u8; 65536];
                let mut answered = false;
                while let Ok(read) = stdout.read(&mut chunk) {
                    if read == 0 {
                        break;
                    }
                    let mut all = output.lock().unwrap();
                    all.extend_from_slice(&chunk[..read]);
                    // A terminal answers the question which terminal it is; qbrow asks once.
                    if !answered && contains(&all, b"\x1b[c") {
                        answered = true;
                        let _ = input.lock().unwrap().write_all(b"\x1b[?62;22c");
                    }
                }
            });
        }
        Self { child, input, output }
    }

    fn write(&self, bytes: &[u8]) {
        let mut input = self.input.lock().unwrap();
        input.write_all(bytes).unwrap();
        input.flush().unwrap();
    }

    fn len(&self) -> usize {
        self.output.lock().unwrap().len()
    }

    /// Waits until the output from `from` on holds `needle`.
    fn wait_for(&self, from: usize, needle: &str, what: &str) {
        let deadline = Instant::now() + PATIENCE;
        while !contains(&self.output.lock().unwrap()[from..], needle.as_bytes()) {
            assert!(Instant::now() < deadline, "no {what} within {PATIENCE:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits until nothing new was drawn for `quiet`.
    fn wait_quiet(&self, quiet: Duration) {
        let deadline = Instant::now() + PATIENCE;
        let (mut last, mut since) = (self.len(), Instant::now());
        while since.elapsed() < quiet {
            assert!(Instant::now() < deadline, "qbrow never stopped drawing");
            std::thread::sleep(Duration::from_millis(50));
            let now = self.len();
            if now != last {
                (last, since) = (now, Instant::now());
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // ctrl+q quits and ends Chromium with it; the kill is only for a test that failed midway.
        let _ = self.input.lock().map(|mut input| input.write_all(b"\x11"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn the_wheel_a_terminal_reports_scrolls_the_page_on_screen() {
    assert!(Path::new(CHROMIUM).exists(), "the tests need Chromium at {CHROMIUM}");
    let scratch = Scratch::new();
    let address = serve_long_page();
    let qbrow = Running::start(&scratch, &address);
    qbrow.wait_for(0, HALF, "picture of the page");
    qbrow.wait_quiet(Duration::from_secs(1));
    let before = qbrow.len();
    // Three turns down over the page, in the middle of the screen's left half; rows and columns
    // count from 1, and the page starts on the third row.
    for _ in 0..3 {
        qbrow.write(b"\x1b[<65;21;11M");
    }
    qbrow.wait_for(before, HALF, "page cells drawn again after the wheel");
    assert!(!scratch.0.join("opened").exists(), "nothing was handed to the desktop");
}
