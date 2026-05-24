//! IPC server — dual UNIX socket model (§12).
//!
//! `.command.sock` — request/response (one connection per command).
//! `.events.sock`  — push-only (clients keep the connection open).
//!
//! Both sockets are non-blocking; the main loop calls `tick()` each iteration.

pub mod commands;
pub mod events;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use self::events::Event;

pub struct IpcServer {
    cmd_listener: UnixListener,
    evt_listener: UnixListener,
    evt_clients: Vec<UnixStream>,
    socket_dir: PathBuf,
    pub instance_sig: String,
}

impl IpcServer {
    pub fn new(instance_sig: &str) -> Result<Self> {
        let runtime_dir =
            std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_owned());
        let socket_dir = PathBuf::from(&runtime_dir)
            .join("griddy")
            .join(instance_sig);
        std::fs::create_dir_all(&socket_dir)?;

        let cmd_path = socket_dir.join(".command.sock");
        let evt_path = socket_dir.join(".events.sock");

        // Remove stale sockets left over from a previous crash.
        let _ = std::fs::remove_file(&cmd_path);
        let _ = std::fs::remove_file(&evt_path);

        let cmd_listener = UnixListener::bind(&cmd_path)?;
        cmd_listener.set_nonblocking(true)?;

        let evt_listener = UnixListener::bind(&evt_path)?;
        evt_listener.set_nonblocking(true)?;

        tracing::info!(
            cmd = %cmd_path.display(),
            evt = %evt_path.display(),
            "IPC sockets ready"
        );

        Ok(IpcServer {
            cmd_listener,
            evt_listener,
            evt_clients: Vec::new(),
            socket_dir,
            instance_sig: instance_sig.to_owned(),
        })
    }

    /// Accept any new event subscribers (non-blocking, drains the backlog).
    pub fn accept_event_subscribers(&mut self) {
        loop {
            match self.evt_listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true).ok();
                    self.evt_clients.push(stream);
                    tracing::debug!("IPC: new event subscriber ({} total)", self.evt_clients.len());
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    tracing::warn!("IPC event accept error: {e}");
                    break;
                }
            }
        }
    }

    /// Try to handle one pending command connection. Returns true if one was processed.
    ///
    /// The caller passes a closure so it can hold `&mut GlobalState` while this
    /// struct is temporarily taken out of `GlobalState::ipc`.
    pub fn try_handle_one<F>(&mut self, mut handler: F) -> bool
    where
        F: FnMut(&str) -> String,
    {
        match self.cmd_listener.accept() {
            Ok((mut stream, _)) => {
                // Short timeout so a slow/silent client doesn't block the loop.
                stream.set_read_timeout(Some(Duration::from_millis(5))).ok();
                let mut line = String::new();
                {
                    let mut reader = BufReader::new(&stream);
                    let _ = reader.read_line(&mut line);
                }
                let cmd = line.trim();
                if !cmd.is_empty() {
                    let response = handler(cmd);
                    let _ = stream.write_all(format!("{response}\n").as_bytes());
                    let _ = stream.flush();
                }
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => false,
            Err(e) => {
                tracing::warn!("IPC command accept error: {e}");
                false
            }
        }
    }

    /// Broadcast a single event to all subscribers, dropping dead connections.
    pub fn broadcast(&mut self, event: &Event) {
        let msg = event.format();
        let bytes = msg.as_bytes();
        self.evt_clients.retain_mut(|s| s.write_all(bytes).is_ok());
    }

    /// Drain a slice of pending events, broadcasting each one.
    pub fn flush_events(&mut self, events: &[Event]) {
        for e in events {
            self.broadcast(e);
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.socket_dir.join(".command.sock"));
        let _ = std::fs::remove_file(self.socket_dir.join(".events.sock"));
    }
}
