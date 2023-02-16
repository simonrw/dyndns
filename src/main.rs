use std::collections::BTreeMap;
use std::error::Error;
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use trust_dns_resolver::config::NameServerConfigGroup;
use trust_dns_resolver::proto::rr::{RecordSet, RecordType};
use trust_dns_server::authority::ZoneType;
use trust_dns_server::client::rr::{LowerName, RrKey};
use trust_dns_server::proto::rr::Name;
use trust_dns_server::store::forwarder::{ForwardAuthority, ForwardConfig};
use trust_dns_server::store::in_memory::InMemoryAuthority;
use trust_dns_server::{authority::Catalog, ServerFuture};

fn forwarder() -> Result<ForwardAuthority, Box<dyn Error>> {
    let config_group = NameServerConfigGroup::google();

    let config = ForwardConfig {
        name_servers: config_group,
        options: None,
    };

    let fwd = ForwardAuthority::try_from_config(Name::from_str(".")?, ZoneType::Forward, &config)?;
    Ok(fwd)
}

fn custom_overrides() -> Result<InMemoryAuthority, Box<dyn Error>> {
    let mut records = BTreeMap::new();

    // add soa
    {
        let key = RrKey {
            name: LowerName::new(&Name::from_str("example.com.")?),
            record_type: RecordType::SOA,
        };
        let mut records = RecordSet::new(&Name::from_str("example.com.")?, RecordType::SOA, 10201);
    }

    // add a records
    {}

    let auth = InMemoryAuthority::new(
        Name::from_str("example.com.").unwrap(),
        records,
        ZoneType::Primary,
        false,
    )?;
    Ok(auth)
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let fwd = forwarder().unwrap();
    let custom = custom_overrides().unwrap();

    let mut catalog = Catalog::new();
    catalog.upsert(
        LowerName::new(&Name::from_str(".").unwrap()),
        Box::new(Arc::new(fwd)),
    );
    catalog.upsert(
        LowerName::new(&Name::from_str("example.com.").unwrap()),
        Box::new(Arc::new(custom)),
    );
    let mut server = ServerFuture::new(catalog);
    let socket = UdpSocket::bind("0.0.0.0:5300").await.unwrap();
    server.register_socket(socket);
    server.block_until_done().await.unwrap();
}
