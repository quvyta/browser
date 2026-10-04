//! The DevTools protocol's wire: a pair of pipes to the browser, numbered calls out, JSON in.
//!
//! Chromium started with `--remote-debugging-pipe` reads calls from its descriptor 3 and writes
//! answers and events to its descriptor 4, each message a JSON text ended by a NUL byte. Nothing
//! listens on a port, so no other process or user on the machine can reach the browser: a
//! DevTools port on 127.0.0.1 would let any of them read every tab, take the cookies and drive
//! the pages.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::Sender;

use serde_json::{Value, json};

use super::tabs::Command;

/// What the socket thread is handed, in the order it happened: the engine's commands and
/// Chromium's messages on one channel, so the thread sleeps until either comes.
pub(super) enum Work {
    /// The engine asks for something.
    Command(Command),
    /// A response or an event from Chromium.
    Message(Value),
    /// The connection is over, and why.
    Closed(String),
}

/// The browser-wide DevTools connection's sending end.
pub(super) struct Wire {
    out: Box<dyn Write + Send>,
    next_id: u64,
}

impl Wire {
    /// The wire that writes calls to `out`, Chromium's descriptor 3.
    pub(super) fn new(out: impl Write + Send + 'static) -> Self {
        Self { out: Box::new(out), next_id: 1 }
    }

    /// Sends `method` with `params`, to the tab behind `session` or to the browser itself, and
    /// returns the call's number for matching its response. A failed write is not reported here:
    /// the connection is then gone, and the reading end says so.
    pub(super) fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut message = serde_json::Map::new();
        message.insert("id".into(), json!(id));
        message.insert("method".into(), json!(method));
        message.insert("params".into(), params);
        if let Some(session) = session {
            message.insert("sessionId".into(), json!(session));
        }
        let mut bytes = Value::Object(message).to_string().into_bytes();
        bytes.push(0);
        let _ = self.out.write_all(&bytes).and_then(|()| self.out.flush());
        id
    }
}

/// Reads Chromium's messages from `input`, its descriptor 4, until it closes, and hands each one
/// over as [`Work::Message`]; the end as [`Work::Closed`]. Reading blocks, so an idle browser
/// costs nothing.
pub(super) fn read(input: impl Read, work: &Sender<Work>) {
    let mut reader = BufReader::new(input);
    let mut message = Vec::new();
    loop {
        message.clear();
        match reader.read_until(0, &mut message) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                let _ = work.send(Work::Closed(error.to_string()));
                return;
            }
        }
        if message.last() == Some(&0) {
            message.pop();
        }
        // The protocol only ever sends JSON; anything else is not worth ending over.
        if let Ok(value) = serde_json::from_slice(&message)
            && work.send(Work::Message(value)).is_err()
        {
            return;
        }
    }
    let _ = work.send(Work::Closed("Chromium closed the DevTools connection".to_owned()));
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn a_call_is_one_json_text_ended_by_a_nul_and_numbered_in_turn() {
        #[derive(Clone, Default)]
        struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let written = Shared::default();
        let mut wire = Wire::new(written.clone());
        assert_eq!(wire.call(None, "Browser.getVersion", json!({})), 1);
        assert_eq!(wire.call(Some("S"), "Page.enable", json!({})), 2);
        let bytes = written.0.lock().unwrap().clone();
        let messages: Vec<Value> = bytes
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| serde_json::from_slice(part).unwrap())
            .collect();
        assert_eq!(bytes.last(), Some(&0));
        assert_eq!(messages[0], json!({ "id": 1, "method": "Browser.getVersion", "params": {} }));
        assert_eq!(messages[1], json!({ "id": 2, "method": "Page.enable", "params": {}, "sessionId": "S" }));
    }

    #[test]
    fn messages_are_read_up_to_each_nul_and_the_end_is_said() {
        let input: &[u8] = b"{\"id\":1,\"result\":{}}\0not json\0{\"method\":\"Page.loadEventFired\"}\0";
        let (work, received) = mpsc::channel();
        read(input, &work);
        let got: Vec<Work> = received.try_iter().collect();
        assert!(matches!(&got[0], Work::Message(value) if value["id"] == 1));
        assert!(matches!(&got[1], Work::Message(value) if value["method"] == "Page.loadEventFired"));
        assert!(matches!(&got[2], Work::Closed(_)));
        assert_eq!(got.len(), 3, "a message that is not JSON is passed over");
    }
}
