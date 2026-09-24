//! The DevTools protocol's wire: one WebSocket to the browser, numbered calls out, JSON in.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

/// How long one read waits for Chromium before the socket thread looks at its commands again.
/// Short enough that a click reaches the page without a felt delay, long enough that an idle
/// browser costs next to nothing.
const POLL: Duration = Duration::from_millis(10);

/// What one read brought.
pub(super) enum Incoming {
    /// A response or an event.
    Message(Value),
    /// Nothing within [`POLL`].
    Idle,
    /// The connection is over, and why.
    Closed(String),
}

/// A browser-wide DevTools connection.
pub(super) struct Wire {
    socket: WebSocket<TcpStream>,
    next_id: u64,
}

impl Wire {
    /// Connects to `address`, a `ws://127.0.0.1:<port>/devtools/browser/<id>` Chromium printed.
    pub(super) fn connect(address: &str) -> Result<Self, String> {
        let host = address
            .strip_prefix("ws://")
            .and_then(|rest| rest.split('/').next())
            .ok_or_else(|| format!("not a DevTools address: {address}"))?;
        let stream = TcpStream::connect(host).map_err(|error| format!("{host}: {error}"))?;
        let (socket, _) = tungstenite::client(address, stream).map_err(|error| format!("{address}: {error}"))?;
        // The read timeout is what lets one thread both wait for Chromium and serve commands;
        // tungstenite keeps a partly read message and goes on with it on the next read.
        socket.get_ref().set_read_timeout(Some(POLL)).map_err(|error| error.to_string())?;
        // Input events are small and each one should leave at once.
        let _ = socket.get_ref().set_nodelay(true);
        Ok(Self { socket, next_id: 1 })
    }

    /// Sends `method` with `params`, to the tab behind `session` or to the browser itself, and
    /// returns the call's number for matching its response. A failed write is not reported here:
    /// the connection is then gone, and the next [`Wire::receive`] says so.
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
        let _ = self.socket.send(Message::text(Value::Object(message).to_string()));
        id
    }

    /// Waits at most [`POLL`] for the next message.
    pub(super) fn receive(&mut self) -> Incoming {
        match self.socket.read() {
            Ok(Message::Text(text)) => match serde_json::from_str(text.as_str()) {
                Ok(value) => Incoming::Message(value),
                // The protocol only ever sends JSON; anything else is not worth ending over.
                Err(_) => Incoming::Idle,
            },
            Ok(Message::Close(_)) => Incoming::Closed("Chromium closed the DevTools connection".to_owned()),
            Ok(_) => Incoming::Idle,
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                Incoming::Idle
            }
            Err(error) => Incoming::Closed(error.to_string()),
        }
    }
}
