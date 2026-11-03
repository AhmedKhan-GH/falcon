//! The Eagle MCP pane: the settings of the MCP server (`falcon_server::mcp`)
//! and what the server is doing now.

use std::ops::RangeInclusive;

use eframe::egui::{self, RichText};
use falcon_server::mcp::{self, RESERVED_PORT};

use super::{Draft, EAGLE_PORT, PORT_SPAN, Page, Prefs, new_token, note, port_error};
use crate::locale::{tr, tr_key, trf};
use crate::theme as t;

/// What the MCP server is doing, as the pane reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Off,
    /// On, with no library open to serve.
    Waiting,
    Running(u16),
    /// On, and every port was taken.
    Failed,
}

impl Prefs {
    /// The ports to try for the MCP server, or None when it is off.
    pub fn mcp_ports(&self) -> Option<RangeInclusive<u16>> {
        let first = if mcp_port_error(self.mcp_port).is_none() { self.mcp_port } else { mcp::DEFAULT_PORT };
        self.mcp_enabled.then_some(first..=first + (PORT_SPAN - 1))
    }

    /// What a running MCP server is told.
    pub fn mcp_options(&self) -> mcp::Options {
        mcp::Options { allow_writes: self.mcp_writes, token: self.mcp_require_token.then(|| self.api_token.clone()) }
    }
}

/// Why a first MCP port is unusable, if it is: the API's rules, and it stays
/// off the port other MCP servers commonly use.
pub fn mcp_port_error(first: u16) -> Option<String> {
    port_error(first).or_else(|| {
        let last = first + (PORT_SPAN - 1);
        (first..=last).contains(&RESERVED_PORT).then(|| trf("{first}–{last} would include port {port}, used by other MCP servers", &[("first", &first), ("last", &last), ("port", &RESERVED_PORT)]))
    })
}

/// Parses the pane's port field.
pub fn parse_mcp_port(text: &str) -> Result<u16, String> {
    let p: u16 = text.trim().parse().map_err(|_| tr("Enter a number from 1024 to 65531").to_string())?;
    match mcp_port_error(p) {
        Some(e) => Err(e),
        None => Ok(p),
    }
}

/// The pane.
pub(super) fn pane(p: &mut Page, d: &mut Draft, port_text: &mut String, status: Status) {
    let pr = &mut d.prefs;
    let parsed = parse_mcp_port(port_text);
    p.block(tr_key("Server"), &["mcp", "model context protocol", "server", "assistant", "ai", "port", "address", "running", "localhost"], |ui| {
        ui.checkbox(&mut pr.mcp_enabled, tr("Run the MCP server while a library is open"));
        note(ui, tr("AI assistants on this Mac can then read the library Falcon has open."));
        ui.add_space(4.0);
        ui.label(match status {
            Status::Off => tr("Not running").to_string(),
            Status::Waiting => tr("Waiting for a library to open").to_string(),
            Status::Running(port) => trf("Running at {address}", &[("address", &address(port))]),
            Status::Failed => tr("Could not start: every port was in use").to_string(),
        });
        ui.add_space(4.0);
        ui.add_enabled_ui(pr.mcp_enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("First port"));
                ui.add(egui::TextEdit::singleline(port_text).desired_width(70.0));
            });
            match &parsed {
                Err(e) => {
                    ui.label(RichText::new(e).size(11.0).color(t::p().icon_red));
                }
                Ok(first) => {
                    note(ui, &trf("falcon tries ports {first} to {last} and never uses Eagle's {eagle}.", &[("first", first), ("last", &(first + PORT_SPAN - 1)), ("eagle", &EAGLE_PORT)]));
                }
            }
        });
    });
    p.block(tr_key("Access"), &["mcp", "write", "read only", "token", "bearer", "password", "trash", "permission"], |ui| {
        note(ui, tr("Assistants can read the library's folders, tags, items and thumbnails. Folders with a password, and the items in them, are never shown."));
        ui.add_space(4.0);
        ui.checkbox(&mut pr.mcp_writes, tr("Allow assistants to change the library"));
        ui.label(RichText::new(tr("Warning: a connected assistant can then add items, change names, tags and notes, create folders and move items to the Trash, and Falcon won't ask you first. Nothing is deleted for good, and Undo reverses each change.")).size(11.0).color(t::p().icon_red));
        note(ui, tr("Assistants see the change to their tools when they reconnect."));
        ui.add_space(4.0);
        if ui.checkbox(&mut pr.mcp_require_token, tr("Require the API token")).changed() && pr.mcp_require_token && pr.api_token.is_empty() {
            pr.api_token = new_token();
        }
        note(ui, tr("Assistants must then send the token from the Developer pane. Without it, any program on this Mac can connect, while web pages are refused."));
    });
    p.block(tr_key("Client configuration"), &["mcp", "client", "config", "json", "copy", "assistant", "url"], |ui| {
        note(ui, tr("Add this to your assistant's MCP settings."));
        let port = match status {
            Status::Running(port) => port,
            _ => parsed.clone().unwrap_or(pr.mcp_port),
        };
        let config = mcp::client_config(port, pr.mcp_require_token.then_some(pr.api_token.as_str()));
        ui.add(egui::Label::new(RichText::new(&config).monospace().size(11.0)).selectable(true));
        if ui.button(tr("Copy")).clicked() {
            ui.ctx().copy_text(config);
        }
    });
}

fn address(port: u16) -> String {
    format!("http://127.0.0.1:{port}{}", mcp::PATH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_and_read_only_by_default() {
        let p = Prefs::default();
        assert!(!p.mcp_enabled && !p.mcp_writes && !p.mcp_require_token);
        assert_eq!(p.mcp_ports(), None);
        assert_eq!(p.mcp_options(), mcp::Options::default());
        let old: Prefs = serde_json::from_str(r#"{"hover_zoom":false}"#).unwrap();
        assert!(!old.mcp_enabled && !old.mcp_writes, "older saved state stays off");
    }

    #[test]
    fn on_uses_its_own_ports() {
        let on = Prefs { mcp_enabled: true, ..Default::default() };
        let ports = on.mcp_ports().unwrap();
        assert_eq!(*ports.start(), mcp::DEFAULT_PORT);
        assert!(!on.api_ports().unwrap().any(|p| ports.contains(&p)), "clear of the HTTP API's ports");
        assert!(!ports.contains(&EAGLE_PORT) && !ports.contains(&RESERVED_PORT));
    }

    #[test]
    fn ports_that_are_refused() {
        assert!(parse_mcp_port("41610").is_ok());
        for bad in ["", "abc", "80", "41595", "41592", "8000", "7996", "65535"] {
            assert!(parse_mcp_port(bad).is_err(), "{bad}");
        }
        // A bad saved port falls back, so the range is never one of those.
        let bad = Prefs { mcp_enabled: true, mcp_port: 8000, ..Default::default() };
        assert_eq!(*bad.mcp_ports().unwrap().start(), mcp::DEFAULT_PORT);
    }

    #[test]
    fn the_token_follows_its_setting() {
        let p = Prefs { api_token: "abc".into(), mcp_writes: true, ..Default::default() };
        assert_eq!(p.mcp_options(), mcp::Options { allow_writes: true, token: None });
        let required = Prefs { mcp_require_token: true, ..p };
        assert_eq!(required.mcp_options().token.as_deref(), Some("abc"));
        // With no token to match, nobody gets in rather than everybody.
        let empty = Prefs { mcp_require_token: true, ..Default::default() };
        assert_eq!(empty.mcp_options().token.as_deref(), Some(""));
    }
}
