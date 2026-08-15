use std::fmt;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::application::{self, ApplicationError};

/// The longest network an automatic scan will sweep. A /24 means at most 254
/// probes per interface; anything larger must be requested with --subnet.
pub(crate) const AUTO_SCAN_MINIMUM_PREFIX: u8 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Subnet {
    network: Ipv4Addr,
    prefix: u8,
}

impl Subnet {
    /// Parse CIDR notation such as `10.42.0.0/24`. Host bits are cleared, so
    /// `10.42.0.71/24` names the same subnet as `10.42.0.0/24`.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let error = || format!("expected CIDR notation such as 10.42.0.0/24, found `{text}`");
        let (address, prefix) = text.split_once('/').ok_or_else(error)?;
        let address = address.trim().parse::<Ipv4Addr>().map_err(|_| error())?;
        let prefix = prefix
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|prefix| *prefix <= 32)
            .ok_or_else(error)?;
        Ok(Self::new(address, prefix))
    }

    fn new(address: Ipv4Addr, prefix: u8) -> Self {
        Self {
            network: Ipv4Addr::from(u32::from(address) & prefix_mask(prefix)),
            prefix,
        }
    }

    /// Derive the connected subnet of an interface address. Returns `None`
    /// for a non-contiguous netmask, which cannot name a CIDR subnet.
    pub(crate) fn from_interface(address: Ipv4Addr, netmask: Ipv4Addr) -> Option<Self> {
        let mask = u32::from(netmask);
        (mask.count_ones() == mask.leading_ones())
            .then(|| Self::new(address, mask.leading_ones() as u8))
    }

    pub(crate) fn prefix(&self) -> u8 {
        self.prefix
    }

    /// Probe candidates. Ordinary subnets exclude the network and broadcast
    /// addresses; /31 and /32 have neither (RFC 3021), so every address is a
    /// host — the integration tests rely on /32 working.
    pub(crate) fn hosts(&self) -> Vec<Ipv4Addr> {
        let network = u32::from(self.network);
        let broadcast = network | !prefix_mask(self.prefix);
        let range = if self.prefix >= 31 {
            network..=broadcast
        } else {
            (network + 1)..=(broadcast - 1)
        };
        range.map(Ipv4Addr::from).collect()
    }
}

impl fmt::Display for Subnet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.network, self.prefix)
    }
}

fn prefix_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix))
    }
}

/// One IPv4 address of a local interface, as reported by the operating
/// system.
pub(crate) struct InterfaceAddress {
    pub(crate) name: String,
    pub(crate) address: Ipv4Addr,
    pub(crate) netmask: Ipv4Addr,
}

/// One subnet to sweep. `excluded` is the scanning host's own address, which
/// automatic mode never probes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScanTarget {
    pub(crate) subnet: Subnet,
    pub(crate) interface: Option<String>,
    pub(crate) excluded: Option<Ipv4Addr>,
}

/// The connected subnets an automatic scan sweeps: non-loopback, contiguous
/// netmask, no larger than /24, first interface wins on duplicates.
pub(crate) fn auto_scan_targets(addresses: Vec<InterfaceAddress>) -> Vec<ScanTarget> {
    let mut targets: Vec<ScanTarget> = Vec::new();
    for interface in addresses {
        if interface.address.is_loopback() {
            continue;
        }
        let Some(subnet) = Subnet::from_interface(interface.address, interface.netmask) else {
            continue;
        };
        if subnet.prefix() < AUTO_SCAN_MINIMUM_PREFIX {
            continue;
        }
        if targets.iter().any(|target| target.subnet == subnet) {
            continue;
        }
        targets.push(ScanTarget {
            subnet,
            interface: Some(interface.name),
            excluded: Some(interface.address),
        });
    }
    targets
}

pub(crate) fn local_scan_targets() -> application::Result<Vec<ScanTarget>> {
    let interfaces =
        if_addrs::get_if_addrs().map_err(ApplicationError::EnumerateNetworkInterfaces)?;
    let addresses = interfaces
        .into_iter()
        .filter_map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(v4) => Some(InterfaceAddress {
                name: interface.name,
                address: v4.ip,
                netmask: v4.netmask,
            }),
            if_addrs::IfAddr::V6(_) => None,
        })
        .collect();
    Ok(auto_scan_targets(addresses))
}

/// A bound on simultaneous connection attempts, well below typical file
/// descriptor limits while keeping a /24 sweep to a couple of batches.
const MAX_CONCURRENT_PROBES: usize = 128;

/// A host that accepted a TCP connection on the probed port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiscoveredHost {
    pub(crate) address: Ipv4Addr,
    pub(crate) port: u16,
    pub(crate) interface: Option<String>,
}

/// Sweep every candidate address of every target. Opening and immediately
/// dropping a stream proves a listener without sending a byte the printer
/// could interpret as ESC/POS data. Failures and timeouts are the normal
/// case for a sweep and are silently skipped.
///
/// `on_progress` is called once up front with `(0, total)` before any probe
/// is spawned, then again with `(done, total)` after each probe completes,
/// ending with `(total, total)`. `total` is the number of probes actually
/// spawned (every target host minus its target's excluded address), so a
/// caller building a progress bar can size it exactly. This module stays
/// free of any UI concern beyond that callback; rendering a bar from it is
/// the caller's job.
pub(crate) async fn scan(
    targets: &[ScanTarget],
    port: u16,
    probe_timeout: Duration,
    mut on_progress: impl FnMut(u64, u64),
) -> Vec<DiscoveredHost> {
    // Counted before spawning so `total` is known, and reported via
    // `on_progress`, before the first probe starts. Counted per target
    // (each target's transient `hosts()` Vec is dropped before the next) so
    // this never holds every target's candidates in memory at once — that
    // matters for large explicit --subnet stacks.
    let total: u64 = targets
        .iter()
        .map(|target| {
            target
                .subnet
                .hosts()
                .into_iter()
                .filter(|address| target.excluded != Some(*address))
                .count() as u64
        })
        .sum();
    on_progress(0, total);

    let limiter = Arc::new(Semaphore::new(MAX_CONCURRENT_PROBES));
    let mut probes = JoinSet::new();
    for target in targets {
        for address in target.subnet.hosts() {
            if target.excluded == Some(address) {
                continue;
            }
            let interface = target.interface.clone();
            let limiter = Arc::clone(&limiter);
            probes.spawn(async move {
                let _permit = limiter
                    .acquire_owned()
                    .await
                    .expect("the probe semaphore is never closed");
                let connected = timeout(probe_timeout, TcpStream::connect((address, port)))
                    .await
                    .is_ok_and(|result| result.is_ok());
                connected.then_some(DiscoveredHost {
                    address,
                    port,
                    interface,
                })
            });
        }
    }

    let mut done = 0u64;
    let mut hosts = Vec::new();
    while let Some(result) = probes.join_next().await {
        if let Ok(Some(host)) = result {
            hosts.push(host);
        }
        done += 1;
        on_progress(done, total);
    }
    // Overlapping explicit --subnet values may find one address twice.
    hosts.sort_by_key(|host| u32::from(host.address));
    hosts.dedup_by_key(|host| host.address);
    hosts
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use super::{DiscoveredHost, InterfaceAddress, ScanTarget, Subnet, auto_scan_targets, scan};

    #[test]
    fn parse_normalizes_host_bits_to_the_network_address() {
        let subnet = Subnet::parse("10.42.0.71/24").expect("a valid CIDR should parse");
        assert_eq!(subnet.to_string(), "10.42.0.0/24");
        assert_eq!(subnet.prefix(), 24);
    }

    #[test]
    fn parse_rejects_text_without_a_prefix() {
        assert!(Subnet::parse("10.42.0.0").is_err());
        assert!(Subnet::parse("10.42.0.0/33").is_err());
        assert!(Subnet::parse("not-an-address/24").is_err());
        assert!(Subnet::parse("10.42.0.0/").is_err());
    }

    #[test]
    fn hosts_exclude_network_and_broadcast_for_ordinary_prefixes() {
        let subnet = Subnet::parse("192.168.7.0/30").expect("a valid CIDR should parse");
        assert_eq!(
            subnet.hosts(),
            vec![Ipv4Addr::new(192, 168, 7, 1), Ipv4Addr::new(192, 168, 7, 2)]
        );
    }

    #[test]
    fn hosts_of_a_24_are_the_254_usable_addresses() {
        let subnet = Subnet::parse("10.42.0.0/24").expect("a valid CIDR should parse");
        let hosts = subnet.hosts();
        assert_eq!(hosts.len(), 254);
        assert_eq!(hosts[0], Ipv4Addr::new(10, 42, 0, 1));
        assert_eq!(hosts[253], Ipv4Addr::new(10, 42, 0, 254));
    }

    #[test]
    fn tiny_subnets_probe_every_address() {
        // /31 and /32 have no network or broadcast address (RFC 3021).
        let single = Subnet::parse("127.0.0.1/32").expect("a valid CIDR should parse");
        assert_eq!(single.hosts(), vec![Ipv4Addr::new(127, 0, 0, 1)]);

        let pair = Subnet::parse("10.0.0.0/31").expect("a valid CIDR should parse");
        assert_eq!(
            pair.hosts(),
            vec![Ipv4Addr::new(10, 0, 0, 0), Ipv4Addr::new(10, 0, 0, 1)]
        );
    }

    #[test]
    fn from_interface_derives_the_connected_subnet() {
        let subnet =
            Subnet::from_interface(Ipv4Addr::new(10, 42, 0, 1), Ipv4Addr::new(255, 255, 255, 0))
                .expect("a contiguous netmask should derive a subnet");
        assert_eq!(subnet.to_string(), "10.42.0.0/24");
    }

    #[test]
    fn from_interface_rejects_a_non_contiguous_netmask() {
        assert!(
            Subnet::from_interface(Ipv4Addr::new(10, 42, 0, 1), Ipv4Addr::new(255, 0, 255, 0),)
                .is_none()
        );
    }

    fn interface(name: &str, address: [u8; 4], netmask: [u8; 4]) -> InterfaceAddress {
        InterfaceAddress {
            name: name.to_owned(),
            address: Ipv4Addr::from(address),
            netmask: Ipv4Addr::from(netmask),
        }
    }

    #[test]
    fn auto_targets_keep_small_connected_subnets_and_remember_the_interface() {
        let targets =
            auto_scan_targets(vec![interface("enx0", [10, 42, 0, 1], [255, 255, 255, 0])]);

        assert_eq!(
            targets,
            vec![ScanTarget {
                subnet: Subnet::parse("10.42.0.0/24").expect("a valid CIDR should parse"),
                interface: Some("enx0".to_owned()),
                excluded: Some(Ipv4Addr::new(10, 42, 0, 1)),
            }]
        );
    }

    #[test]
    fn auto_targets_skip_loopback_and_networks_larger_than_a_24() {
        let targets = auto_scan_targets(vec![
            interface("lo", [127, 0, 0, 1], [255, 0, 0, 0]),
            interface("docker0", [172, 17, 0, 1], [255, 255, 0, 0]),
        ]);

        assert!(targets.is_empty());
    }

    #[test]
    fn auto_targets_deduplicate_identical_subnets() {
        let targets = auto_scan_targets(vec![
            interface("eth0", [10, 42, 0, 1], [255, 255, 255, 0]),
            interface("eth0:1", [10, 42, 0, 2], [255, 255, 255, 0]),
        ]);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].interface.as_deref(), Some("eth0"));
    }

    #[tokio::test]
    async fn scan_reports_a_listening_host_and_ignores_closed_ports() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("an ephemeral loopback port should bind");
        let port = listener
            .local_addr()
            .expect("the listener should report its address")
            .port();
        let target = ScanTarget {
            subnet: Subnet::parse("127.0.0.1/32").expect("a valid CIDR should parse"),
            interface: Some("lo".to_owned()),
            excluded: None,
        };

        let hosts = scan(
            std::slice::from_ref(&target),
            port,
            Duration::from_secs(1),
            |_, _| {},
        )
        .await;
        assert_eq!(
            hosts,
            vec![DiscoveredHost {
                address: Ipv4Addr::new(127, 0, 0, 1),
                port,
                interface: Some("lo".to_owned()),
            }]
        );

        drop(listener);
        let hosts = scan(&[target], port, Duration::from_secs(1), |_, _| {}).await;
        assert!(hosts.is_empty());
    }

    #[tokio::test]
    async fn scan_never_probes_the_excluded_own_address() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("an ephemeral loopback port should bind");
        let port = listener
            .local_addr()
            .expect("the listener should report its address")
            .port();
        let target = ScanTarget {
            subnet: Subnet::parse("127.0.0.1/32").expect("a valid CIDR should parse"),
            interface: None,
            excluded: Some(Ipv4Addr::new(127, 0, 0, 1)),
        };

        assert!(
            scan(&[target], port, Duration::from_secs(1), |_, _| {})
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn scan_reports_progress_from_zero_to_total_monotonically() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("an ephemeral loopback port should bind");
        let port = listener
            .local_addr()
            .expect("the listener should report its address")
            .port();
        let target = ScanTarget {
            subnet: Subnet::parse("127.0.0.1/32").expect("a valid CIDR should parse"),
            interface: None,
            excluded: None,
        };
        // Two targets probing the same address spawn two independent probes
        // (the final results are what dedup, not the probe count), giving a
        // known total of 2 without depending on the host's own interfaces.
        let targets = vec![target.clone(), target];

        let mut calls = Vec::new();
        let hosts = scan(&targets, port, Duration::from_secs(1), |done, total| {
            calls.push((done, total));
        })
        .await;

        assert_eq!(hosts.len(), 1, "duplicate address should still dedup");
        assert_eq!(
            calls.first(),
            Some(&(0, 2)),
            "first call reports (0, total)"
        );
        assert_eq!(
            calls.last(),
            Some(&(2, 2)),
            "final call reports (total, total)"
        );
        assert!(
            calls.iter().all(|(_, total)| *total == 2),
            "total never changes mid-scan"
        );
        assert!(
            calls.windows(2).all(|pair| pair[0].0 <= pair[1].0),
            "done is monotonically nondecreasing: {calls:?}"
        );
        assert_eq!(calls.len(), 3, "one call up front plus one per probe");
    }
}
