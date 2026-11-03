//! Runs the MCP server (`falcon_server::mcp`) for the shown library, as
//! Preferences says, and takes its writes into the undo history.

use std::ops::RangeInclusive;

use falcon_server::mcp::{self, WriteKind};

use super::App;
use super::actions::push_capped;
use crate::locale::tr;
use crate::prefs::mcp::Status;

/// The running server.
#[derive(Default)]
pub(crate) struct McpState {
    server: Option<mcp::Handle>,
    /// The ports the server was started on, so changing them restarts it.
    ports: Option<RangeInclusive<u16>>,
    failed: bool,
}

impl McpState {
    fn status(&self, enabled: bool) -> Status {
        match &self.server {
            Some(h) => Status::Running(h.port),
            None if !enabled => Status::Off,
            None if self.failed => Status::Failed,
            None => Status::Waiting,
        }
    }
}

impl App {
    /// Brings the server in line with the preferences and the shown library:
    /// starts or stops it, or hands a running one the library and options.
    /// It only ever gets the library the app has open.
    pub(crate) fn follow_mcp(&mut self) {
        match (self.ws.lib.clone(), self.prefs.mcp_ports()) {
            (Some(lib), Some(ports)) => match &self.mcp.server {
                Some(h) if self.mcp.ports.as_ref() == Some(&ports) => {
                    h.set_library(lib);
                    h.set_options(self.prefs.mcp_options());
                }
                _ => {
                    // The old one goes first: it may hold the wanted port.
                    self.mcp.server = None;
                    let ctx = self.ctx.clone();
                    let started = mcp::start(lib, ports.clone(), self.prefs.mcp_options(), move || ctx.request_repaint());
                    if let Err(e) = &started {
                        crate::errlog::error(&format!("MCP server not started: {e}"));
                    }
                    self.mcp.failed = started.is_err();
                    self.mcp.server = started.ok();
                    self.mcp.ports = Some(ports);
                }
            },
            _ => self.mcp = McpState::default(),
        }
        self.show_mcp_status();
    }

    /// Tells the Preferences window what the server is doing.
    pub(crate) fn show_mcp_status(&mut self) {
        self.prefs_window.mcp_status = self.mcp.status(self.prefs.mcp_enabled);
    }

    /// Adds the server's writes to the undo history of the library each one
    /// changed, as a frame's edits are added.
    pub(crate) fn take_mcp_undo(&mut self) {
        let Some(server) = &self.mcp.server else { return };
        for mut step in server.take_undo() {
            step.transaction.label = match step.kind {
                WriteKind::AddItem => tr("Add item"),
                WriteKind::UpdateItem => tr("Edit item"),
                WriteKind::CreateFolder => tr("New folder"),
                WriteKind::MoveToTrash => tr("Move to Trash"),
            }
            .to_string();
            let target = std::iter::once(&mut self.ws).chain(self.others.parked_mut()).find(|w| w.undo_root.as_deref() == Some(step.root.as_path()));
            if let Some(ws) = target {
                push_capped(&mut ws.undo, step.transaction);
                ws.redo.clear();
            }
        }
    }
}
