use time::OffsetDateTime;

use crate::application;

use super::list;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) updated_at: OffsetDateTime,
    pub(crate) warning: Option<String>,
    pub(crate) printers: Vec<list::Printer>,
}

pub(crate) async fn collect_once(request: list::Request) -> application::Result<Snapshot> {
    let response = list::execute_with_observer(request, |_| {}).await?;
    Ok(snapshot_from_response(response, OffsetDateTime::now_utc()))
}

fn snapshot_from_response(response: list::Response, updated_at: OffsetDateTime) -> Snapshot {
    Snapshot {
        updated_at,
        warning: None,
        printers: response.printers,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::features::printers::list::{ConnectionFacts, NetworkConnectionFacts};
    use crate::features::printers::{Availability, Transport};

    #[tokio::test]
    async fn one_shot_snapshot_wraps_the_structured_inventory() {
        let response = list::Response {
            config_path: PathBuf::from("/tmp/printers.toml"),
            printers: vec![network_printer("kitchen", Availability::Connected)],
        };
        let now = OffsetDateTime::from_unix_timestamp(1_787_754_730)
            .expect("the fixed test timestamp should be valid");

        let snapshot = snapshot_from_response(response, now);

        assert_eq!(snapshot.updated_at, now);
        assert_eq!(snapshot.warning, None);
        assert_eq!(snapshot.printers[0].name, "kitchen");
    }

    fn network_printer(name: &str, availability: Availability) -> list::Printer {
        list::Printer {
            name: name.to_owned(),
            transport: Transport::Network,
            availability,
            profile: None,
            connection: ConnectionFacts::Network(NetworkConnectionFacts {
                host: "127.0.0.1".to_owned(),
                port: 9100,
            }),
        }
    }
}
