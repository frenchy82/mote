//! The addresses a scan covers: the private IPv4 networks this computer is
//! on, leaving out its own addresses, the tunnels and the containers, and
//! any others the user names.

use std::fmt;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::str::FromStr;

use nix::ifaddrs::getifaddrs;
use nix::net::if_::InterfaceFlags;

/// The widest network scanned. A wider one is scanned only for the /24
/// around this computer: the rest could be thousands of addresses nobody
/// asked to have knocked on.
const WIDEST_PREFIX: u32 = 24;

/// The widest network the user can name. Those are asked for, but this one
/// already takes up to 18 minutes to scan, and each step wider doubles it.
const WIDEST_NAMED_PREFIX: u32 = 16;

/// What the container engines name the bridges to their containers, whose
/// networks are wide and hold no desktops. Those of virtual machines stay.
const CONTAINER_BRIDGES: [&str; 6] = ["docker", "br-", "veth", "podman", "cni", "flannel"];

/// A network interface's IPv4 address.
#[derive(Debug, Clone)]
pub(super) struct Interface {
    pub(super) name: String,
    pub(super) address: Ipv4Addr,
    pub(super) netmask: Ipv4Addr,
    /// Up, with a link.
    pub(super) running: bool,
    /// A link with one other end, as a VPN's: not a network with neighbors.
    pub(super) point_to_point: bool,
}

pub(super) fn interfaces() -> Vec<Interface> {
    let Ok(addresses) = getifaddrs() else {
        return Vec::new();
    };
    addresses
        .filter_map(|interface| {
            let ip = |storage: Option<nix::sys::socket::SockaddrStorage>| {
                storage.and_then(|s| s.as_sockaddr_in().map(|a| a.ip()))
            };
            let flags = interface.flags;
            Some(Interface {
                address: ip(interface.address)?,
                netmask: ip(interface.netmask)?,
                running: flags.contains(InterfaceFlags::IFF_UP | InterfaceFlags::IFF_RUNNING),
                point_to_point: flags.contains(InterfaceFlags::IFF_POINTOPOINT),
                name: interface.interface_name,
            })
        })
        .collect()
}

/// A range of addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Network {
    base: Ipv4Addr,
    prefix: u32,
}

/// Why text doesn't name a network that can be scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkError {
    NotANetwork,
    /// Trying every address of someone else's network is what abuse reports
    /// are made of.
    NotPrivate,
    TooWide,
}

impl Network {
    /// The network of `address`, narrowed to [`WIDEST_PREFIX`].
    fn around(address: Ipv4Addr, netmask: Ipv4Addr) -> Self {
        let prefix = netmask.to_bits().count_ones().max(WIDEST_PREFIX);
        Self::with_prefix(address, prefix)
    }

    fn with_prefix(address: Ipv4Addr, prefix: u32) -> Self {
        let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
        Self {
            base: Ipv4Addr::from_bits(address.to_bits() & mask),
            prefix,
        }
    }

    /// How many addresses a scan tries in it.
    pub(super) fn size(self) -> usize {
        self.hosts().count()
    }

    /// Its addresses, without the first and last where those stand for the
    /// network and its broadcast.
    fn hosts(self) -> impl Iterator<Item = Ipv4Addr> {
        let size = 1u32 << (32 - self.prefix);
        let base = self.base.to_bits();
        let (first, last) = if size > 2 {
            (base + 1, base + size - 2)
        } else {
            (base, base + size - 1)
        };
        (first..=last).map(Ipv4Addr::from_bits)
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.prefix == 32 {
            write!(f, "{}", self.base)
        } else {
            write!(f, "{}/{}", self.base, self.prefix)
        }
    }
}

/// `192.168.2.0/24`, or one address alone. An address inside the network
/// will do for its first, as `ip address` shows them.
impl FromStr for Network {
    type Err = NetworkError;

    fn from_str(text: &str) -> Result<Self, NetworkError> {
        let (address, prefix) = text.split_once('/').unwrap_or((text, "32"));
        let address: Ipv4Addr = address.parse().map_err(|_| NetworkError::NotANetwork)?;
        // u32's parser would take a sign.
        if !prefix.bytes().all(|b| b.is_ascii_digit()) {
            return Err(NetworkError::NotANetwork);
        }
        let prefix: u32 = prefix.parse().map_err(|_| NetworkError::NotANetwork)?;
        if prefix > 32 {
            return Err(NetworkError::NotANetwork);
        }
        if !is_local(address) {
            return Err(NetworkError::NotPrivate);
        }
        if prefix < WIDEST_NAMED_PREFIX {
            return Err(NetworkError::TooWide);
        }
        Ok(Self::with_prefix(address, prefix))
    }
}

/// The ranges no one else's network can be in: the private ones, link-local,
/// and the shared range that Tailscale and carriers' NATs use.
fn is_local(address: Ipv4Addr) -> bool {
    let [first, second, ..] = address.octets();
    address.is_private() || address.is_link_local() || (first == 100 && second & 0xC0 == 64)
}

/// The networks to scan: `named` after those this computer is on. Of those,
/// only private ranges: a public address on an interface doesn't make what
/// is around it this computer's own network.
pub(super) fn to_scan(interfaces: &[Interface], named: &[Network]) -> Vec<Network> {
    let mut networks = Vec::new();
    for interface in interfaces {
        let container = CONTAINER_BRIDGES
            .iter()
            .any(|prefix| interface.name.starts_with(prefix));
        if !interface.running
            || interface.point_to_point
            || container
            || !interface.address.is_private()
        {
            continue;
        }
        let network = Network::around(interface.address, interface.netmask);
        if !networks.contains(&network) {
            networks.push(network);
        }
    }
    for network in named {
        if !networks.contains(network) {
            networks.push(*network);
        }
    }
    networks
}

/// Every address and port to try, each once. This computer's own addresses
/// aren't among them: there's no use in a remote desktop of the one in
/// front of you.
pub(super) fn targets(networks: &[Network], own: &[Ipv4Addr], ports: &[u16]) -> Vec<SocketAddrV4> {
    let mut targets: Vec<SocketAddrV4> = networks
        .iter()
        .flat_map(|network| network.hosts())
        .filter(|host| !own.contains(host))
        .flat_map(|host| ports.iter().map(move |port| SocketAddrV4::new(host, *port)))
        .collect();
    targets.sort_unstable();
    targets.dedup();
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface(name: &str, address: [u8; 4], netmask: [u8; 4]) -> Interface {
        Interface {
            name: name.into(),
            address: address.into(),
            netmask: netmask.into(),
            running: true,
            point_to_point: false,
        }
    }

    fn names(interfaces: &[Interface]) -> Vec<String> {
        to_scan(interfaces, &[])
            .iter()
            .map(Network::to_string)
            .collect()
    }

    #[test]
    fn a_network_wider_than_a_slash_24_is_narrowed_around_this_computer() {
        let wide = interface("eth0", [10, 7, 200, 9], [255, 255, 0, 0]);
        assert_eq!(names(&[wide]), ["10.7.200.0/24"]);
        let narrow = interface("eth0", [192, 168, 1, 130], [255, 255, 255, 192]);
        assert_eq!(names(&[narrow]), ["192.168.1.128/26"]);
    }

    #[test]
    fn only_private_networks_with_neighbors_are_scanned() {
        let mask = [255, 255, 255, 0];
        let mut down = interface("eth1", [10, 0, 2, 5], mask);
        down.running = false;
        let mut vpn = interface("wg0", [10, 0, 3, 5], mask);
        vpn.point_to_point = true;
        assert!(
            names(&[
                down,
                vpn,
                interface("lo", [127, 0, 0, 1], [255, 0, 0, 0]),
                interface("docker0", [172, 17, 0, 1], [255, 255, 0, 0]),
                interface("br-5a1e1d4d6044", [172, 18, 0, 1], [255, 255, 0, 0]),
                interface("eth0", [8, 8, 8, 8], mask),
                interface("eth0", [169, 254, 3, 3], [255, 255, 0, 0]),
            ])
            .is_empty()
        );
        assert_eq!(
            names(&[interface("virbr0", [192, 168, 122, 1], mask)]),
            ["192.168.122.0/24"],
            "virtual machines are worth finding"
        );
    }

    #[test]
    fn two_interfaces_on_one_network_scan_it_once() {
        let mask = [255, 255, 255, 0];
        let networks = names(&[
            interface("eth0", [10, 0, 1, 23], mask),
            interface("wlan0", [10, 0, 1, 24], mask),
        ]);
        assert_eq!(networks, ["10.0.1.0/24"]);
    }

    #[test]
    fn named_networks_come_after_this_computer_s_and_only_once() {
        let eth0 = interface("eth0", [10, 0, 1, 23], [255, 255, 255, 0]);
        let named: Vec<Network> = ["10.0.2.0/24", "10.0.1.0/24"]
            .iter()
            .map(|n| n.parse().unwrap())
            .collect();
        let networks: Vec<String> = to_scan(&[eth0], &named)
            .iter()
            .map(Network::to_string)
            .collect();
        assert_eq!(networks, ["10.0.1.0/24", "10.0.2.0/24"]);
    }

    #[test]
    fn a_named_network_is_scanned_without_any_interface_on_it() {
        let named = ["192.168.2.0/30".parse().unwrap()];
        let targets = targets(&to_scan(&[], &named), &[], &[3389]);
        assert_eq!(
            targets,
            [
                "192.168.2.1:3389".parse().unwrap(),
                "192.168.2.2:3389".parse().unwrap()
            ]
        );
    }

    #[test]
    fn overlapping_networks_try_each_address_once() {
        let wide: Network = "10.0.0.0/23".parse().unwrap();
        let narrow: Network = "10.0.1.0/24".parse().unwrap();
        assert_eq!(targets(&[wide, narrow], &[], &[3389]).len(), 510);
    }

    #[test]
    fn text_names_a_network_or_one_address() {
        let shown = |text: &str| text.parse::<Network>().unwrap().to_string();
        assert_eq!(shown("192.168.2.0/24"), "192.168.2.0/24");
        assert_eq!(shown("192.168.2.77/24"), "192.168.2.0/24");
        assert_eq!(shown("10.20.0.0/16"), "10.20.0.0/16");
        assert_eq!(shown("10.0.1.33"), "10.0.1.33");
        assert_eq!(shown("10.0.1.33/32"), "10.0.1.33");
        assert_eq!(shown("172.31.255.1/30"), "172.31.255.0/30");
        assert_eq!(shown("100.100.1.1/24"), "100.100.1.0/24");
        assert_eq!(shown("169.254.3.0/24"), "169.254.3.0/24");
    }

    #[test]
    fn text_that_can_t_be_scanned_says_why() {
        for text in [
            "",
            "printer",
            "10.0.1",
            "10.0.1.0/",
            "10.0.1.0/33",
            "10.0.1.0/+24",
            "10.0.1.0/-1",
            "/24",
            "10.0.1.0/24/24",
            "2001:db8::/64",
        ] {
            assert_eq!(
                text.parse::<Network>(),
                Err(NetworkError::NotANetwork),
                "{text}"
            );
        }
        for text in [
            "8.8.8.0/24",
            "1.1.1.1",
            "172.32.0.0/24",
            "100.128.0.0/24",
            "127.0.0.1",
        ] {
            assert_eq!(
                text.parse::<Network>(),
                Err(NetworkError::NotPrivate),
                "{text}"
            );
        }
        for text in [
            "10.0.0.0/8",
            "192.168.0.0/15",
            "172.16.0.0/12",
            "10.0.0.0/0",
        ] {
            assert_eq!(
                text.parse::<Network>(),
                Err(NetworkError::TooWide),
                "{text}"
            );
        }
    }

    #[test]
    fn what_is_saved_reads_back_the_same() {
        for text in ["192.168.2.0/24", "10.0.1.33"] {
            let network: Network = text.parse().unwrap();
            assert_eq!(network.to_string().parse(), Ok(network));
        }
    }

    #[test]
    fn targets_leave_out_this_computer_and_the_broadcast() {
        let own = Ipv4Addr::new(10, 0, 1, 23);
        let network = Network::around(own, Ipv4Addr::new(255, 255, 255, 0));
        let targets = targets(&[network, network], &[own], &[3389, 3390]);
        assert_eq!(targets.len(), 253 * 2);
        assert!(!targets.iter().any(|t| t.ip() == &own));
        assert_eq!(targets[0], "10.0.1.1:3389".parse().unwrap());
        assert_eq!(targets[1], "10.0.1.1:3390".parse().unwrap());
        assert_eq!(
            targets[targets.len() - 1],
            "10.0.1.254:3390".parse().unwrap()
        );
    }

    #[test]
    fn the_narrowest_networks_have_no_broadcast_to_skip() {
        let hosts = |netmask: [u8; 4]| {
            Network::around(Ipv4Addr::new(10, 0, 0, 5), netmask.into())
                .hosts()
                .collect::<Vec<_>>()
        };
        assert_eq!(hosts([255, 255, 255, 255]), [Ipv4Addr::new(10, 0, 0, 5)]);
        assert_eq!(
            hosts([255, 255, 255, 254]),
            [Ipv4Addr::new(10, 0, 0, 4), Ipv4Addr::new(10, 0, 0, 5)]
        );
    }
}
