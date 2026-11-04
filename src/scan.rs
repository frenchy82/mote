//! Finding RDP servers on the local networks. Nothing here touches a
//! widget: the interface runs [`run`] as a command and hears back through
//! [`Event`]s. A server is only ever asked for its certificate, never signed
//! in to, and the scan runs only when asked for: it sends a connection to
//! every address in range, which a network's monitoring can notice.

mod network;
mod probe;

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use gtk::gio;
use gtk::gio::prelude::*;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub(crate) use network::Network;

use crate::connection::DEFAULT_PORT;
use crate::rdp::server::{Server, certificate_name};

/// RDP's own port, and the one GNOME's Desktop Sharing moves to when Remote
/// Login has that.
pub(crate) const PORTS: [u16; 2] = [DEFAULT_PORT, 3390];

/// Probes under way at once: most wait out their timeout on an address with
/// nothing there, so this sets how long a scan takes, about four seconds for
/// each network, while staying far from the limit of 1024 open files.
const AT_ONCE: usize = 192;

/// How long a name gets to resolve, either way.
const LOOKUP_FOR: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(crate) enum Event {
    /// The networks about to be scanned; none when there is nothing to scan.
    Networks(Vec<Network>),
    /// How much of the scan is done, from 0 to 1.
    Progress(f64),
    Found(Box<Found>),
    Done,
}

/// An RDP server that answered.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub(crate) address: SocketAddrV4,
    /// What a connection to it should use: a name that resolves back to the
    /// address, which outlasts a lease from DHCP, or else the address.
    pub(crate) host: String,
    /// What it calls itself where `host` has to be the address; often empty.
    pub(crate) name: String,
    pub(crate) server: Option<Server>,
    /// A connection reaches this already.
    pub(crate) saved: bool,
}

/// Scan, sending what turns up to `events`. `saved` holds the host and port
/// of each connection there is, to mark what is already saved.
pub(crate) async fn run(saved: Vec<(String, u16)>, events: impl Fn(Event)) {
    let interfaces = network::interfaces();
    let networks = network::to_scan(&interfaces);
    events(Event::Networks(networks.clone()));
    let saved = Arc::new(resolve(saved).await);
    let own: Vec<Ipv4Addr> = interfaces.iter().map(|i| i.address).collect();
    let targets = network::targets(&networks, &own, &PORTS);
    let total = targets.len();

    let limit = Arc::new(Semaphore::new(AT_ONCE));
    let mut probes = JoinSet::new();
    for address in targets {
        let (limit, saved) = (limit.clone(), saved.clone());
        probes.spawn(async move {
            let _turn = limit.acquire_owned().await.ok()?;
            let answer = probe::probe(address).await?;
            Some(identify(address, answer, &saved).await)
        });
    }
    let mut done = 0;
    while let Some(result) = probes.join_next().await {
        done += 1;
        events(Event::Progress(done as f64 / total as f64));
        if let Ok(Some(found)) = result {
            events(Event::Found(Box::new(found)));
        }
    }
    events(Event::Done);
}

/// The addresses the saved connections reach. A tunnel or a gateway's
/// connections aren't among them: their host is seen from the far end.
async fn resolve(saved: Vec<(String, u16)>) -> HashSet<SocketAddrV4> {
    let mut lookups = JoinSet::new();
    for (host, port) in saved {
        lookups.spawn(async move { addresses(&host, port).await });
    }
    let mut found = HashSet::new();
    while let Some(result) = lookups.join_next().await {
        found.extend(result.unwrap_or_default());
    }
    found
}

async fn addresses(host: &str, port: u16) -> Vec<SocketAddrV4> {
    let lookup = tokio::net::lookup_host((host, port));
    match tokio::time::timeout(LOOKUP_FOR, lookup).await {
        Ok(Ok(found)) => found
            .filter_map(|address| match address {
                SocketAddr::V4(address) => Some(address),
                SocketAddr::V6(_) => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

async fn identify(
    address: SocketAddrV4,
    answer: probe::Answer,
    saved: &HashSet<SocketAddrV4>,
) -> Found {
    let der = answer.certificate.as_deref();
    let reversed = reverse(*address.ip()).await;
    let resolves = match &reversed {
        Some(name) => addresses(name, address.port()).await.contains(&address),
        None => false,
    };
    let (host, name) = match reversed {
        Some(name) if resolves => (name, String::new()),
        name => {
            let name = name.or_else(|| {
                der.and_then(certificate_name)
                    .filter(|name| is_host_name(name))
            });
            (address.ip().to_string(), name.unwrap_or_default())
        }
    };
    Found {
        address,
        host,
        name,
        server: der.and_then(Server::from_certificate),
        saved: saved.contains(&address),
    }
}

/// The name DNS gives the address. GIO's resolver goes through the system's,
/// which blocks, so it gets a thread of the runtime's.
async fn reverse(ip: Ipv4Addr) -> Option<String> {
    let lookup = tokio::task::spawn_blocking(move || {
        gio::Resolver::default()
            .lookup_by_address(
                &gio::InetAddress::from(IpAddr::V4(ip)),
                gio::Cancellable::NONE,
            )
            .ok()
            .map(|name| name.to_string())
    });
    let name = tokio::time::timeout(LOOKUP_FOR, lookup)
        .await
        .ok()?
        .ok()??;
    let name = name.trim_end_matches('.');
    is_host_name(name).then(|| name.to_owned())
}

/// Whether `name` can stand for a computer. It comes from the network, and
/// systemd's resolver offers names like `_gateway` of its own.
fn is_host_name(name: &str) -> bool {
    name.len() <= 253
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !name.eq_ignore_ascii_case("localhost")
        && name.parse::<Ipv4Addr>().is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_computer_s_name_counts_as_one() {
        for name in ["carbon", "dev-fedora.kepler.lan", "PC01"] {
            assert!(is_host_name(name), "{name}");
        }
        for name in [
            "",
            "_gateway",
            "localhost",
            "10.0.1.33",
            "a b",
            "-x",
            "<b>",
            "user@host",
        ] {
            assert!(!is_host_name(name), "{name}");
        }
    }
}
