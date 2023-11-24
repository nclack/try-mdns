use std::{net::SocketAddr, time::Duration};

use anyhow::Result;
use clap::Parser;
use log::info;
use mdns_sd::ServiceInfo;
use smol::net::UdpSocket;

#[derive(Parser, Debug, Clone)]
#[command(
    author,
    version,
    help_template = "\
{before-help}{name} {version} by {author}
{about}

{usage-heading}
{usage}

{all-args}{after-help}"
)]
/// An experiment in discovery
///
/// I'm trying to play with mdns service discovery to enable zeroconf networking
/// between devices on a local network.
struct Config {
    /// Instance name
    instance_name: String,

    /// Service name
    #[arg(short, default_value = "_example._udp")]
    service_name: String,

    /// Port
    #[arg(short, default_value = "0")]
    port: u16,

    /// Key=Value properties to share with peers.
    #[arg(value_parser=parse_key_val::<String,String>)]
    properties: Vec<(String, String)>,
}

struct UdpService {
    sock: UdpSocket,
}

impl UdpService {
    async fn new(config: &Config) -> Result<Self> {
        let sock = UdpSocket::bind(("127.0.0.1", config.port)).await?;
        info!("UdpSocket at local addr {:?}", sock.local_addr());
        Ok(Self { sock })
    }

    async fn run(self) -> Result<()> {
        let mut buf = vec![0u8; 20];

        info!("LISTENING on {:?}", self.sock.local_addr());
        loop {
            // Receive a single datagram message.
            // If `buf` is too small to hold the entire message, it will be cut off.
            let (n, addr) = self.sock.recv_from(&mut buf).await?;
            info!("RECV {:?} FROM {:}", &buf[0..n], addr);
        }
    }
}

struct DiscoveryService {
    config: Config,
    service_addr: SocketAddr,
}

impl DiscoveryService {
    fn new(config: &Config, service_addr: SocketAddr) -> Self {
        Self {
            config: config.clone(),
            service_addr,
        }
    }

    async fn run(self) -> Result<()> {
        info!("STARTING DISCOVERY");
        let config = &self.config;
        let service_name = format!("{}.local.", config.service_name);
        let info = ServiceInfo::new(
            &service_name,
            &config.instance_name,
            gethostname::gethostname().to_str().unwrap(),
            "",
            self.service_addr.port(),
            &config.properties[..],
        )?
        .enable_addr_auto();

        info!("Registering {}", info.get_fullname());

        let service = mdns_sd::ServiceDaemon::new()?;
        service.register(info)?;

        let receiver = service.browse(&service_name)?;

        // let receiver = service.monitor()?;

        while let Ok(event) = receiver.recv_async().await {
            info!("Event: {event:?}");
            match event {
                mdns_sd::ServiceEvent::ServiceResolved(info) => {
                    info!("ServiceResolved");
                    todo!()
                }
                _ => (),
            }
            futures_timer::Delay::new(Duration::from_millis(500)).await;
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    smol::block_on(async {
        dotenv::dotenv().ok();
        pretty_env_logger::init();
        let config = Config::parse();

        info!("Hi there! {config:?}");

        info!("Spinning up UDP listener");
        let udp_service = UdpService::new(&config).await?;

        info!("Spinning up mDNS discovery");
        let discovery_service = DiscoveryService::new(&config, udp_service.sock.local_addr()?);

        smol::future::try_zip(udp_service.run(), discovery_service.run()).await?;

        Ok(())
    })
}

/// Parse a single key-value pair
fn parse_key_val<T, U>(
    s: &str,
) -> Result<(T, U), Box<dyn std::error::Error + Send + Sync + 'static>>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
    U: std::str::FromStr,
    U::Err: std::error::Error + Send + Sync + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}
