//! Checks that the local API can be reached the way browser extensions
//! reach it, through `localhost`. A proxy or VPN that redirects `localhost`
//! makes the extensions fail even though the server runs. Pure connection
//! attempts (no HTTP request), made on a background thread a moment after
//! the server starts.

use std::net::{TcpStream, ToSocketAddrs};
use std::ops::RangeInclusive;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use crate::locale::{tr, tr_key, trf};

/// How long after the server starts the check runs.
const DELAY: Duration = Duration::from_secs(1);
/// How long each connection attempt may take.
const TIMEOUT: Duration = Duration::from_millis(500);

/// Title of the alert both warnings use.
pub const TITLE: &str = tr_key("Local API unavailable");

/// The title as shown, in the current language; [`TITLE`] stays the key.
pub fn title() -> &'static str {
    tr(TITLE)
}

/// Whether `shown` is this warning's title, in any language.
pub fn is_title(shown: &str) -> bool {
    shown == TITLE || shown == title()
}

/// Whether any address `localhost` resolves to (IPv4 or IPv6 loopback)
/// accepts a connection on `port`.
pub fn localhost_reaches(port: u16, timeout: Duration) -> bool {
    let Ok(addrs) = ("localhost", port).to_socket_addrs() else { return false };
    addrs.into_iter().any(|a| a.ip().is_loopback() && TcpStream::connect_timeout(&a, timeout).is_ok())
}

/// The warning for a server that couldn't start on any port of `ports`.
pub fn start_failed_message(ports: &RangeInclusive<u16>) -> String {
    trf(
        "The local API couldn't start: ports {first}–{last} are all in use. Apps and browser extensions that talk to Falcon won't reach it. Choose another first port in Preferences ▸ Developer.",
        &[("first", ports.start()), ("last", ports.end())],
    )
}

/// The warning for a server that runs but isn't reached through `localhost`.
pub fn unreachable_message(port: u16) -> String {
    trf(
        "The local API is running on port {port} but can't be reached through localhost, so browser extensions can't reach Falcon. A proxy or VPN may be redirecting localhost.",
        &[("port", &port)],
    )
}

/// A check in flight.
#[derive(Default)]
pub struct Check {
    rx: Option<Receiver<(u16, bool)>>,
}

impl Check {
    /// Starts checking `port` in a moment, dropping any check before it.
    pub fn start(&mut self, port: u16, repaint: impl Fn() + Send + 'static) {
        let (tx, rx) = channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            std::thread::sleep(DELAY);
            let _ = tx.send((port, localhost_reaches(port, TIMEOUT)));
            repaint();
        });
    }

    /// Forgets a check that hasn't reported.
    pub fn cancel(&mut self) {
        self.rx = None;
    }

    /// The finished check's port and result, once.
    pub fn poll(&mut self) -> Option<(u16, bool)> {
        let got = self.rx.as_ref()?.try_recv().ok()?;
        self.rx = None;
        Some(got)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn a_listening_port_is_reached() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // `localhost` may resolve to ::1 only on some machines; the listener
        // is on IPv4, so skip when it doesn't resolve to 127.0.0.1.
        let has_v4 = ("localhost", port).to_socket_addrs().is_ok_and(|mut a| a.any(|a| a.is_ipv4()));
        if has_v4 {
            assert!(localhost_reaches(port, Duration::from_secs(1)));
        }
    }

    #[test]
    fn a_closed_port_is_not_reached() {
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert!(!localhost_reaches(port, Duration::from_millis(300)));
    }

    #[test]
    fn messages_name_the_ports() {
        let m = start_failed_message(&(41596..=41600));
        assert!(m.starts_with("The local API couldn't start: ports 41596–41600 are all in use."));
        assert!(m.ends_with("Choose another first port in Preferences ▸ Developer."));
        assert!(unreachable_message(41597).contains("running on port 41597 but can't be reached through localhost"));
    }

    #[test]
    fn a_check_reports_once() {
        let mut c = Check::default();
        assert_eq!(c.poll(), None);
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        c.start(port, || {});
        let mut got = None;
        for _ in 0..50 {
            got = got.or(c.poll());
            if got.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(got.map(|g| g.0), Some(port));
        assert_eq!(c.poll(), None);
    }
}
