//! Network status probe for the cloud chain (v0.3.0, issue 20).
//!
//! `NetworkStatus` implements `NetworkStatusPort` with a short TCP probe
//! and deterministic env overrides for tests and offline rigs:
//! `SUSURRO_OFFLINE=1` forces offline, `SUSURRO_ONLINE=1` forces online.
//! The default probe dials 8.8.8.8:53 with a 3s timeout. No DNS, no TLS,
//! no credentials. A failed probe means offline, never an error.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;
use susurro_core::ports::{NetworkState, NetworkStatusPort};

pub const PROBE_HOST: &str = "8.8.8.8";
pub const PROBE_PORT: u16 = 53;
pub const PROBE_TIMEOUT_SECS: u64 = 3;

#[derive(Debug, Clone)]
pub struct NetworkStatus {
    pub timeout: Duration,
    pub probe: Option<String>,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(PROBE_TIMEOUT_SECS),
            probe: None,
        }
    }
}

impl NetworkStatus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_probe(host: &str, port: u16, timeout_secs: u64) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.clamp(1, 30)),
            probe: Some(format!("{host}:{port}")),
        }
    }

    pub fn is_online(&self) -> bool {
        matches!(self.status(), NetworkState::Online)
    }

    fn probe_addr(&self) -> Option<SocketAddr> {
        let target = self
            .probe
            .clone()
            .unwrap_or_else(|| format!("{PROBE_HOST}:{PROBE_PORT}"));
        target.to_socket_addrs().ok()?.next()
    }
}

impl NetworkStatusPort for NetworkStatus {
    fn status(&self) -> NetworkState {
        if std::env::var("SUSURRO_OFFLINE")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            return NetworkState::Offline;
        }
        if std::env::var("SUSURRO_ONLINE")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            return NetworkState::Online;
        }
        match self.probe_addr() {
            Some(addr) => match TcpStream::connect_timeout(&addr, self.timeout) {
                Ok(_) => NetworkState::Online,
                Err(_) => NetworkState::Offline,
            },
            None => NetworkState::Offline,
        }
    }
}

/// Fixed-state network double for tests and CI: always returns the
/// state it was constructed with, with no socket and no env reads.
pub struct MockNetwork {
    state: NetworkState,
}

impl MockNetwork {
    /// Fixed answer for every `status` call.
    pub fn new(state: NetworkState) -> Self {
        Self { state }
    }

    /// Always-online double.
    pub fn online() -> Self {
        Self::new(NetworkState::Online)
    }

    /// Always-offline double.
    pub fn offline() -> Self {
        Self::new(NetworkState::Offline)
    }
}

impl NetworkStatusPort for MockNetwork {
    fn status(&self) -> NetworkState {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn offline_override_wins() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("SUSURRO_OFFLINE", "1");
        std::env::remove_var("SUSURRO_ONLINE");
        assert!(matches!(
            NetworkStatus::new().status(),
            NetworkState::Offline
        ));
        std::env::remove_var("SUSURRO_OFFLINE");
    }

    #[test]
    fn online_override_wins() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("SUSURRO_ONLINE", "1");
        std::env::remove_var("SUSURRO_OFFLINE");
        assert!(matches!(
            NetworkStatus::new().status(),
            NetworkState::Online
        ));
        assert!(NetworkStatus::new().is_online());
        std::env::remove_var("SUSURRO_ONLINE");
    }

    #[test]
    fn mock_network_online_returns_online() {
        assert!(matches!(
            MockNetwork::new(NetworkState::Online).status(),
            NetworkState::Online
        ));
        assert!(matches!(
            MockNetwork::online().status(),
            NetworkState::Online
        ));
    }

    #[test]
    fn mock_network_offline_returns_offline() {
        assert!(matches!(
            MockNetwork::new(NetworkState::Offline).status(),
            NetworkState::Offline
        ));
        assert!(matches!(
            MockNetwork::offline().status(),
            NetworkState::Offline
        ));
    }

    #[test]
    fn closed_port_reads_offline_without_panic() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("SUSURRO_OFFLINE");
        std::env::remove_var("SUSURRO_ONLINE");
        let probe = NetworkStatus::with_probe("127.0.0.1", 1, 2);
        assert!(matches!(probe.status(), NetworkState::Offline));
    }

    #[test]
    fn local_listener_reads_online() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("SUSURRO_OFFLINE");
        std::env::remove_var("SUSURRO_ONLINE");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let probe = NetworkStatus::with_probe("127.0.0.1", port, 2);
        assert!(matches!(probe.status(), NetworkState::Online));
    }
}
