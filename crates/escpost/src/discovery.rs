use std::collections::HashSet;
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

/// The longest network an explicitly named subnet may cover. Asking for a
/// subnet is a deliberate act, so this is far more permissive than the
/// automatic limit — but it is not unbounded. A /16 is already 65,534 probes
/// and minutes of sweeping; larger is a mistake or an attack, not a request.
///
/// The bound also protects the scan's own cancellation: `Subnet::hosts`
/// materializes every candidate address, and neither that allocation nor the
/// loop that spawns the probes contains an await point. Once a sweep of a /0
/// starts, nothing can interrupt it — not a disconnected browser, not a
/// dropped `JoinSet` — so it must never be allowed to start.
pub(crate) const EXPLICIT_SCAN_MINIMUM_PREFIX: u8 = 16;

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

    /// Whether this subnet covers an address, used to exclude the scanning
    /// host's own addresses from any target that contains them.
    pub(crate) fn contains(&self, address: Ipv4Addr) -> bool {
        u32::from(address) & prefix_mask(self.prefix) == u32::from(self.network)
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

/// One subnet to sweep. `excluded` holds the scanning host's own addresses
/// inside this subnet, which no scan ever probes: the workbench is not a
/// printer, and its own `serve` listener must never be discovered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScanTarget {
    pub(crate) subnet: Subnet,
    pub(crate) interface: Option<String>,
    pub(crate) excluded: Vec<Ipv4Addr>,
}

/// Why an adapter is not swept automatically. Reported rather than dropped:
/// silence here reads as "this machine has no networks".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkipReason {
    /// Larger than `AUTO_SCAN_MINIMUM_PREFIX` allows.
    TooLarge,
    /// A non-contiguous netmask cannot name a CIDR subnet.
    UnusableNetmask,
}

/// An adapter left out of an automatic sweep. Exists so the omission can be
/// told to the user instead of silently vanishing: without this, a machine
/// whose only interface is too large to sweep reports "no networks" as if it
/// had none, rather than naming the one it declined to scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SkippedInterface {
    pub(crate) name: String,
    pub(crate) subnet: Option<Subnet>,
    pub(crate) reason: SkipReason,
}

impl SkippedInterface {
    /// Why this adapter was left out, naming it and the subnet it covers when
    /// the netmask named one. Shared by the CLI's pre-scan skip lines, the
    /// "nothing to scan" error, and the workbench's disabled rows, so a user
    /// reads one explanation for an omission rather than several differently
    /// worded ones.
    ///
    /// The reason only. What to do about it is the calling interface's own
    /// wording, because the answers genuinely differ: the terminal names a
    /// flag (`cli_hint`), while the workbench points at the custom-network
    /// field sitting beside the row.
    pub(crate) fn describe(&self) -> String {
        match (self.reason, self.subnet) {
            (SkipReason::TooLarge, Some(subnet)) => {
                format!("{} ({subnet}): larger than /24", self.name)
            }
            (SkipReason::TooLarge, None) | (SkipReason::UnusableNetmask, _) => {
                format!(
                    "{}: its netmask does not name a scannable subnet",
                    self.name
                )
            }
        }
    }

    /// The terminal's remedy for this omission, appended to `describe()` by
    /// the CLI adapter alone. `None` when no subnet could be derived: there is
    /// then nothing to pass to `--subnet`, so there is no advice to give.
    pub(crate) fn cli_hint(&self) -> Option<String> {
        match (self.reason, self.subnet) {
            (SkipReason::TooLarge, Some(subnet)) => Some(format!("scan it with --subnet {subnet}")),
            (SkipReason::TooLarge, None) | (SkipReason::UnusableNetmask, _) => None,
        }
    }
}

/// Every skipped adapter's `describe()`, joined into one reportable clause.
/// Empty when nothing was skipped, so a caller can splice it straight into a
/// message without a special case for "nothing to say". Reasons only: this
/// clause ends up inside `NoDiscoverableSubnets`, which both interfaces
/// report, and each appends its own guidance to it.
pub(crate) fn describe_skipped(skipped: &[SkippedInterface]) -> String {
    skipped
        .iter()
        .map(SkippedInterface::describe)
        .collect::<Vec<_>>()
        .join("; ")
}

/// The local addresses a subnet covers, in the order the operating system
/// reported them. Every scan target's exclusions come from here, so automatic
/// and explicit targets exclude exactly the same addresses.
///
/// Membership is decided by the subnet being scanned, not by the netmask an
/// address was reported with: an alias carrying a /32 netmask on a scanned
/// /24 is still this machine's address inside that /24.
fn local_addresses_within(
    subnet: Subnet,
    addresses: &[InterfaceAddress],
) -> Vec<&InterfaceAddress> {
    addresses
        .iter()
        .filter(|interface| subnet.contains(interface.address))
        .collect()
}

/// Automatic detection with its omissions kept. Loopback is not reported: it is
/// never a candidate, so naming it would be noise.
pub(crate) fn detect_networks(
    addresses: Vec<InterfaceAddress>,
) -> (Vec<ScanTarget>, Vec<SkippedInterface>) {
    let mut targets: Vec<ScanTarget> = Vec::new();
    let mut skipped = Vec::new();
    for interface in &addresses {
        if interface.address.is_loopback() {
            continue;
        }
        let Some(subnet) = Subnet::from_interface(interface.address, interface.netmask) else {
            skipped.push(SkippedInterface {
                name: interface.name.clone(),
                subnet: None,
                reason: SkipReason::UnusableNetmask,
            });
            continue;
        };
        if subnet.prefix() < AUTO_SCAN_MINIMUM_PREFIX {
            skipped.push(SkippedInterface {
                name: interface.name.clone(),
                subnet: Some(subnet),
                reason: SkipReason::TooLarge,
            });
            continue;
        }
        // A second address on a known subnet adds no target; the first
        // interface on it names the one target this subnet gets.
        if !targets.iter().any(|target| target.subnet == subnet) {
            targets.push(ScanTarget {
                subnet,
                interface: Some(interface.name.clone()),
                excluded: Vec::new(),
            });
        }
    }
    // Exclusions are filled in once the targets are known, from every local
    // address the target subnet covers rather than from the addresses that
    // derived it. An address whose own netmask names a different subnet — a
    // /32 alias, a virtual address — still sits inside the target and must
    // not be probed.
    for target in &mut targets {
        target.excluded = local_addresses_within(target.subnet, &addresses)
            .into_iter()
            .map(|interface| interface.address)
            .collect();
    }
    (targets, skipped)
}

/// How many addresses a scan of these targets will probe. Both the progress
/// bar and the pre-scan announcement are sized from this.
pub(crate) fn probe_count(targets: &[ScanTarget]) -> u64 {
    targets
        .iter()
        .map(|target| {
            target
                .subnet
                .hosts()
                .into_iter()
                .filter(|address| !target.excluded.contains(address))
                .count() as u64
        })
        .sum()
}

/// Every IPv4 address this machine holds, loopback included. Both target
/// builders need it: automatic detection to find subnets, explicit subnets to
/// find what must not be probed.
pub(crate) fn local_interface_addresses() -> application::Result<Vec<InterfaceAddress>> {
    let interfaces =
        if_addrs::get_if_addrs().map_err(ApplicationError::EnumerateNetworkInterfaces)?;
    Ok(interfaces
        .into_iter()
        .filter_map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(v4) => Some(InterfaceAddress {
                name: interface.name,
                address: v4.ip,
                netmask: v4.netmask,
            }),
            if_addrs::IfAddr::V6(_) => None,
        })
        .collect())
}

/// Targets for subnets the developer named. A named subnet the machine sits on
/// is treated exactly like an automatically detected one: same interface label,
/// same self-exclusion. A subnet elsewhere gets neither.
///
/// This is the one place explicit subnets become scannable work, so it is also
/// where `EXPLICIT_SCAN_MINIMUM_PREFIX` is enforced — every caller, terminal
/// or browser, inherits the same refusal with the same wording.
pub(crate) fn explicit_scan_targets(
    subnets: &[Subnet],
    addresses: &[InterfaceAddress],
) -> application::Result<Vec<ScanTarget>> {
    subnets
        .iter()
        .map(|subnet| {
            if subnet.prefix() < EXPLICIT_SCAN_MINIMUM_PREFIX {
                return Err(ApplicationError::SubnetTooLargeToScan(subnet.to_string()));
            }
            let local = local_addresses_within(*subnet, addresses);
            Ok(ScanTarget {
                subnet: *subnet,
                interface: local.first().map(|interface| interface.name.clone()),
                excluded: local.iter().map(|interface| interface.address).collect(),
            })
        })
        .collect()
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

/// What a sweep reports while it runs.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ScanEvent<'a> {
    Progress {
        completed: u64,
        total: u64,
    },
    /// A host accepted a connection. Reported as it happens so a caller can
    /// show results before the sweep finishes.
    ///
    /// Fired at most once per address, even when overlapping `ScanTarget`s
    /// probe the same host twice: the set of addresses this reports equals
    /// the set in `scan`'s returned `Vec<DiscoveredHost>`. A streaming
    /// consumer and a caller that only reads the return value must see the
    /// same discovered hosts.
    Found(&'a DiscoveredHost),
}

/// Sweep every candidate address of every target. Opening and immediately
/// dropping a stream proves a listener without sending a byte the printer
/// could interpret as ESC/POS data. Failures and timeouts are the normal
/// case for a sweep and are silently skipped.
///
/// `on_event` receives `Progress { completed: 0, total }` once up front
/// before any probe is spawned, a `Found` for each newly discovered address
/// as it happens (see `ScanEvent::Found` for the one-event-per-address
/// guarantee this keeps even when targets overlap), and a `Progress` after
/// every probe completes (found or not), ending with
/// `Progress { completed: total, total }`. `total` is the number of probes
/// actually spawned (every target host minus its target's excluded
/// address), so a caller building a progress bar can size it exactly. This
/// module stays free of any UI concern beyond that callback; rendering from
/// it is the caller's job.
pub(crate) async fn scan(
    targets: &[ScanTarget],
    port: u16,
    probe_timeout: Duration,
    mut on_event: impl FnMut(ScanEvent<'_>),
) -> Vec<DiscoveredHost> {
    // Counted before spawning so `total` is known, and reported via
    // `on_event`, before the first probe starts.
    let total = probe_count(targets);
    on_event(ScanEvent::Progress {
        completed: 0,
        total,
    });

    let limiter = Arc::new(Semaphore::new(MAX_CONCURRENT_PROBES));
    let mut probes = JoinSet::new();
    for target in targets {
        for address in target.subnet.hosts() {
            if target.excluded.contains(&address) {
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
    // Overlapping explicit --subnet values may probe one address twice;
    // `announced` keeps `Found` to one event per address so a streaming
    // consumer sees exactly what the deduped return value below reports.
    // Bounded by hosts that actually answered, not by probes spawned.
    let mut announced = HashSet::new();
    while let Some(result) = probes.join_next().await {
        if let Ok(Some(host)) = result {
            if announced.insert(host.address) {
                on_event(ScanEvent::Found(&host));
            }
            hosts.push(host);
        }
        done += 1;
        on_event(ScanEvent::Progress {
            completed: done,
            total,
        });
    }
    // The returned ordering is what the CLI renders and must not change.
    hosts.sort_by_key(|host| u32::from(host.address));
    hosts.dedup_by_key(|host| host.address);
    hosts
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use super::{
        ApplicationError, DiscoveredHost, EXPLICIT_SCAN_MINIMUM_PREFIX, InterfaceAddress,
        ScanEvent, ScanTarget, SkipReason, SkippedInterface, Subnet, describe_skipped,
        detect_networks, explicit_scan_targets, probe_count, scan,
    };

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

    #[test]
    fn a_subnet_contains_only_its_own_addresses() {
        let subnet = Subnet::parse("10.42.0.0/24").expect("a valid CIDR should parse");

        assert!(subnet.contains(Ipv4Addr::new(10, 42, 0, 71)));
        assert!(!subnet.contains(Ipv4Addr::new(10, 43, 0, 71)));
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
        let (targets, skipped) =
            detect_networks(vec![interface("enx0", [10, 42, 0, 1], [255, 255, 255, 0])]);

        assert_eq!(
            targets,
            vec![ScanTarget {
                subnet: Subnet::parse("10.42.0.0/24").expect("a valid CIDR should parse"),
                interface: Some("enx0".to_owned()),
                excluded: vec![Ipv4Addr::new(10, 42, 0, 1)],
            }]
        );
        assert!(skipped.is_empty());
    }

    #[test]
    fn auto_targets_skip_loopback_and_networks_larger_than_a_24() {
        let (targets, skipped) = detect_networks(vec![
            interface("lo", [127, 0, 0, 1], [255, 0, 0, 0]),
            interface("docker0", [172, 17, 0, 1], [255, 255, 0, 0]),
        ]);

        assert!(targets.is_empty());
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "docker0");
        assert_eq!(skipped[0].reason, SkipReason::TooLarge);
    }

    #[test]
    fn auto_targets_deduplicate_identical_subnets() {
        let (targets, _) = detect_networks(vec![
            interface("eth0", [10, 42, 0, 1], [255, 255, 255, 0]),
            interface("eth0:1", [10, 42, 0, 2], [255, 255, 255, 0]),
        ]);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].interface.as_deref(), Some("eth0"));
    }

    #[test]
    fn automatic_targets_exclude_every_local_address_of_one_subnet() {
        let (targets, _) = detect_networks(vec![
            InterfaceAddress {
                name: "enx0".to_owned(),
                address: Ipv4Addr::new(10, 42, 0, 71),
                netmask: Ipv4Addr::new(255, 255, 255, 0),
            },
            InterfaceAddress {
                name: "enx0:1".to_owned(),
                address: Ipv4Addr::new(10, 42, 0, 72),
                netmask: Ipv4Addr::new(255, 255, 255, 0),
            },
        ]);

        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].excluded,
            vec![Ipv4Addr::new(10, 42, 0, 71), Ipv4Addr::new(10, 42, 0, 72)]
        );
    }

    #[test]
    fn automatic_targets_exclude_a_local_address_whose_own_netmask_names_another_subnet() {
        // A /32 alias — a virtual address, a failover address — derives its
        // own single-address subnet, but it is still this machine's address
        // inside the /24 the sweep covers.
        let (targets, _) = detect_networks(vec![
            interface("eth0", [10, 42, 0, 1], [255, 255, 255, 0]),
            interface("eth0:1", [10, 42, 0, 9], [255, 255, 255, 255]),
        ]);

        let swept = targets
            .iter()
            .find(|target| target.subnet.prefix() == 24)
            .expect("the /24 should be a scan target");
        assert_eq!(
            swept.excluded,
            vec![Ipv4Addr::new(10, 42, 0, 1), Ipv4Addr::new(10, 42, 0, 9)]
        );
        assert_eq!(probe_count(std::slice::from_ref(swept)), 252);
    }

    #[test]
    fn detection_reports_an_adapter_whose_subnet_is_too_large_to_sweep() {
        let (targets, skipped) = detect_networks(vec![
            InterfaceAddress {
                name: "enx0".to_owned(),
                address: Ipv4Addr::new(10, 42, 0, 71),
                netmask: Ipv4Addr::new(255, 255, 255, 0),
            },
            InterfaceAddress {
                name: "enp5s0".to_owned(),
                address: Ipv4Addr::new(10, 0, 0, 5),
                netmask: Ipv4Addr::new(255, 255, 0, 0),
            },
        ]);

        assert_eq!(targets.len(), 1);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "enp5s0");
        assert_eq!(skipped[0].reason, SkipReason::TooLarge);
        assert_eq!(
            skipped[0].subnet.map(|subnet| subnet.to_string()),
            Some("10.0.0.0/16".to_owned())
        );
    }

    #[test]
    fn detection_never_reports_loopback_as_skipped() {
        let (_, skipped) = detect_networks(vec![InterfaceAddress {
            name: "lo".to_owned(),
            address: Ipv4Addr::new(127, 0, 0, 1),
            netmask: Ipv4Addr::new(255, 0, 0, 0),
        }]);

        assert!(skipped.is_empty());
    }

    #[test]
    fn detection_reports_an_adapter_whose_netmask_is_not_contiguous() {
        let (targets, skipped) = detect_networks(vec![InterfaceAddress {
            name: "weird0".to_owned(),
            address: Ipv4Addr::new(10, 42, 0, 1),
            netmask: Ipv4Addr::new(255, 0, 255, 0),
        }]);

        assert!(targets.is_empty());
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "weird0");
        assert_eq!(skipped[0].reason, SkipReason::UnusableNetmask);
        assert_eq!(skipped[0].subnet, None);
    }

    #[test]
    fn describe_names_the_adapter_and_its_subnet_for_a_too_large_adapter() {
        let skipped = SkippedInterface {
            name: "enp5s0".to_owned(),
            subnet: Some(Subnet::parse("10.0.0.0/16").expect("a valid CIDR should parse")),
            reason: SkipReason::TooLarge,
        };

        assert_eq!(skipped.describe(), "enp5s0 (10.0.0.0/16): larger than /24");
    }

    /// The reason travels to every interface, so it must not name a flag the
    /// browser has no way to pass. The flag lives in `cli_hint`, which only
    /// the terminal adapter reads.
    #[test]
    fn describe_leaves_the_remedy_to_the_interface_and_cli_hint_names_the_flag() {
        let too_large = SkippedInterface {
            name: "enp5s0".to_owned(),
            subnet: Some(Subnet::parse("10.0.0.0/16").expect("a valid CIDR should parse")),
            reason: SkipReason::TooLarge,
        };
        let unusable = SkippedInterface {
            name: "weird0".to_owned(),
            subnet: None,
            reason: SkipReason::UnusableNetmask,
        };

        assert!(!too_large.describe().contains("--subnet"));
        assert_eq!(
            too_large.cli_hint().as_deref(),
            Some("scan it with --subnet 10.0.0.0/16")
        );
        // Nothing to pass to the flag, so there is no advice to give.
        assert_eq!(unusable.cli_hint(), None);
    }

    #[test]
    fn describe_names_the_adapter_without_a_subnet_for_an_unusable_netmask() {
        let skipped = SkippedInterface {
            name: "weird0".to_owned(),
            subnet: None,
            reason: SkipReason::UnusableNetmask,
        };

        assert_eq!(
            skipped.describe(),
            "weird0: its netmask does not name a scannable subnet"
        );
    }

    #[test]
    fn describe_skipped_joins_every_adapter_with_a_semicolon() {
        let skipped = vec![
            SkippedInterface {
                name: "enp5s0".to_owned(),
                subnet: Some(Subnet::parse("10.0.0.0/16").expect("a valid CIDR should parse")),
                reason: SkipReason::TooLarge,
            },
            SkippedInterface {
                name: "weird0".to_owned(),
                subnet: None,
                reason: SkipReason::UnusableNetmask,
            },
        ];

        assert_eq!(
            describe_skipped(&skipped),
            "enp5s0 (10.0.0.0/16): larger than /24; \
             weird0: its netmask does not name a scannable subnet"
        );
    }

    #[test]
    fn describe_skipped_is_empty_when_nothing_was_skipped() {
        assert_eq!(describe_skipped(&[]), "");
    }

    #[test]
    fn the_probe_count_subtracts_every_excluded_address() {
        let targets = vec![ScanTarget {
            subnet: Subnet::parse("10.42.0.0/24").expect("a valid CIDR should parse"),
            interface: None,
            excluded: vec![Ipv4Addr::new(10, 42, 0, 71), Ipv4Addr::new(10, 42, 0, 72)],
        }];

        assert_eq!(probe_count(&targets), 252);
    }

    #[test]
    fn an_explicit_subnet_takes_the_label_and_exclusions_of_a_matching_adapter() {
        let addresses = vec![InterfaceAddress {
            name: "enx0".to_owned(),
            address: Ipv4Addr::new(10, 42, 0, 71),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
        }];
        let subnets = vec![Subnet::parse("10.42.0.0/24").expect("a valid CIDR should parse")];

        let targets = explicit_scan_targets(&subnets, &addresses)
            .expect("a /24 is well within the explicit scan limit");

        assert_eq!(targets[0].interface.as_deref(), Some("enx0"));
        assert_eq!(targets[0].excluded, vec![Ipv4Addr::new(10, 42, 0, 71)]);
    }

    #[test]
    fn an_explicit_subnet_this_machine_is_not_on_has_no_label_and_no_exclusions() {
        let addresses = vec![InterfaceAddress {
            name: "enx0".to_owned(),
            address: Ipv4Addr::new(10, 42, 0, 71),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
        }];
        let subnets = vec![Subnet::parse("10.9.0.0/24").expect("a valid CIDR should parse")];

        let targets = explicit_scan_targets(&subnets, &addresses)
            .expect("a /24 is well within the explicit scan limit");

        assert_eq!(targets[0].interface, None);
        assert!(targets[0].excluded.is_empty());
    }

    #[test]
    fn an_explicit_loopback_subnet_still_excludes_the_loopback_address() {
        let addresses = vec![InterfaceAddress {
            name: "lo".to_owned(),
            address: Ipv4Addr::new(127, 0, 0, 1),
            netmask: Ipv4Addr::new(255, 0, 0, 0),
        }];
        let subnets = vec![Subnet::parse("127.0.0.0/24").expect("a valid CIDR should parse")];

        let targets = explicit_scan_targets(&subnets, &addresses)
            .expect("a /24 is well within the explicit scan limit");

        assert_eq!(targets[0].excluded, vec![Ipv4Addr::new(127, 0, 0, 1)]);
    }

    #[test]
    fn an_explicit_subnet_larger_than_the_limit_is_refused_rather_than_swept() {
        let subnets = vec![Subnet::parse("0.0.0.0/0").expect("a valid CIDR should parse")];

        let error = explicit_scan_targets(&subnets, &[])
            .expect_err("a /0 is four billion probes and must be refused");

        assert!(matches!(error, ApplicationError::SubnetTooLargeToScan(_)));
        assert_eq!(
            error.to_string(),
            "subnet 0.0.0.0/0 is too large to scan (at most /16)"
        );
    }

    #[test]
    fn an_explicit_subnet_one_bit_wider_than_the_limit_is_refused() {
        // The bound is only doing its job if the subnet just outside it is
        // refused: a /0 would also be refused by a far looser limit.
        let subnets = vec![Subnet::parse("10.0.0.0/15").expect("a valid CIDR should parse")];

        let error = explicit_scan_targets(&subnets, &[])
            .expect_err("a /15 is wider than the explicit scan limit");

        assert!(matches!(error, ApplicationError::SubnetTooLargeToScan(_)));
        assert_eq!(
            error.to_string(),
            "subnet 10.0.0.0/15 is too large to scan (at most /16)"
        );
    }

    #[test]
    fn an_explicit_subnet_at_the_limit_is_accepted() {
        let subnets = vec![Subnet::parse("10.0.0.0/16").expect("a valid CIDR should parse")];

        let targets = explicit_scan_targets(&subnets, &[])
            .expect("a /16 is the largest explicitly scannable subnet");

        assert_eq!(targets[0].subnet.prefix(), EXPLICIT_SCAN_MINIMUM_PREFIX);
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
            excluded: Vec::new(),
        };

        let hosts = scan(
            std::slice::from_ref(&target),
            port,
            Duration::from_secs(1),
            |_| {},
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
        let hosts = scan(&[target], port, Duration::from_secs(1), |_| {}).await;
        assert!(hosts.is_empty());
    }

    #[tokio::test]
    async fn scan_reports_a_found_event_for_a_listening_host_as_it_is_discovered() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("an ephemeral loopback port should bind");
        let port = listener
            .local_addr()
            .expect("the listener should report its address")
            .port();
        let target = ScanTarget {
            subnet: Subnet::parse("127.0.0.1/32").expect("a valid CIDR should parse"),
            interface: Some("lo".to_owned()),
            excluded: Vec::new(),
        };

        let mut found = Vec::new();
        scan(
            std::slice::from_ref(&target),
            port,
            Duration::from_secs(1),
            |event| {
                if let ScanEvent::Found(host) = event {
                    found.push(host.clone());
                }
            },
        )
        .await;

        assert_eq!(
            found,
            vec![DiscoveredHost {
                address: Ipv4Addr::new(127, 0, 0, 1),
                port,
                interface: Some("lo".to_owned()),
            }],
            "a Found event should carry the discovered host as soon as it answers"
        );
    }

    #[tokio::test]
    async fn scan_reports_a_found_event_at_most_once_per_address_even_with_overlapping_targets() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("an ephemeral loopback port should bind");
        let port = listener
            .local_addr()
            .expect("the listener should report its address")
            .port();
        // Two targets both covering 127.0.0.1: overlapping explicit
        // --subnet values (e.g. 10.0.0.0/25 and 10.0.0.0/24) probe such a
        // host twice, once per target, so this reproduces that at scan()'s
        // level rather than relying on real overlapping subnets.
        let target = ScanTarget {
            subnet: Subnet::parse("127.0.0.1/32").expect("a valid CIDR should parse"),
            interface: Some("lo".to_owned()),
            excluded: Vec::new(),
        };
        let targets = vec![target.clone(), target];

        let mut found = Vec::new();
        let hosts = scan(&targets, port, Duration::from_secs(1), |event| {
            if let ScanEvent::Found(host) = event {
                found.push(host.clone());
            }
        })
        .await;

        assert_eq!(
            found.len(),
            1,
            "one address answering twice must still fire Found once: {found:?}"
        );
        assert_eq!(
            found
                .into_iter()
                .map(|host| host.address)
                .collect::<Vec<_>>(),
            hosts
                .into_iter()
                .map(|host| host.address)
                .collect::<Vec<_>>(),
            "the addresses announced through Found must equal the deduped return value"
        );
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
            excluded: vec![Ipv4Addr::new(127, 0, 0, 1)],
        };

        assert!(
            scan(&[target], port, Duration::from_secs(1), |_| {})
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
            excluded: Vec::new(),
        };
        // Two targets probing the same address spawn two independent probes
        // (the final results are what dedup, not the probe count), giving a
        // known total of 2 without depending on the host's own interfaces.
        let targets = vec![target.clone(), target];

        let mut calls = Vec::new();
        let hosts = scan(&targets, port, Duration::from_secs(1), |event| {
            if let ScanEvent::Progress { completed, total } = event {
                calls.push((completed, total));
            }
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
