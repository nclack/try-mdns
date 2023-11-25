use std::{
    fmt::Write,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use anyhow::{anyhow, Result};
use clap::Parser;
use log::info;
use mdns_sd::ServiceInfo;
use smol::{
    channel::{bounded, Receiver, Sender},
    net::UdpSocket,
};

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

struct Message {
    dst: SocketAddr,
    buf: [u8; 1 << 10],
    n: usize,
}

impl Write for Message {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let Message {
            dst: _,
            ref mut buf,
            ref mut n,
        } = self;
        for (i, c) in s.bytes().enumerate() {
            buf[*n + i] = c;
        }
        *n += s.len();
        Ok(())
    }
}

struct UdpService {
    sock: UdpSocket,
    rx: Receiver<Message>,
}

impl UdpService {
    async fn new(config: &Config) -> Result<(Self, Sender<Message>)> {
        // select the address to bind to
        let addr = if_addrs::get_if_addrs()?
            .into_iter()
            .filter(|iface| !iface.is_loopback() && !iface.is_link_local() && iface.ip().is_ipv4())
            .map(|iface| iface.ip())
            .filter(|ip| match ip {
                IpAddr::V4(v4) => v4.octets()[0] == 10,
                IpAddr::V6(_v6) => false,
            })
            .next()
            .ok_or(anyhow!("Failed to select network interface"))?;

        let sock = UdpSocket::bind((addr, config.port)).await?;
        info!("UdpSocket at local addr {:?}", sock.local_addr());
        let (tx, rx) = bounded(10);
        Ok((Self { sock, rx }, tx))
    }

    async fn run(self) -> Result<()> {
        let Self { sock, rx } = self;

        info!("LISTENING on {:?}", sock.local_addr());
        smol::future::try_zip(
            async {
                loop {
                    let Message { dst, buf, n } = rx.recv().await?;
                    let s = String::from_utf8_lossy(&buf[0..n]);
                    info!("SEND message to {} \"{}\"", dst, s);
                    sock.send_to(&buf, dst).await?;
                }
                #[allow(unreachable_code)]
                Ok(())
            },
            async {
                // Receive a single datagram message.
                // If `buf` is too small to hold the entire message, it will be cut off.
                loop {
                    info!("Listening for messages");
                    let mut buf = vec![0u8; 1 << 10];
                    let (n, addr) = sock.recv_from(&mut buf).await?;
                    let s = String::from_utf8_lossy(&buf[0..n]);
                    info!("RECV \"{}\" FROM {:}", s, addr);
                }
                #[allow(unreachable_code)]
                Ok::<_, anyhow::Error>(())
            },
        )
        .await?;
        Ok(())
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

    async fn run(self, tx: Sender<Message>) -> Result<()> {
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
            // info!("Event: {event:?}");
            match event {
                mdns_sd::ServiceEvent::ServiceResolved(info) => {
                    for ip in info.get_addresses_v4().into_iter() {
                        info!("ServiceResolved");

                        let mut msg = Message {
                            dst: SocketAddr::from((*ip, info.get_port())),
                            buf: [0; 1024],
                            n: 0,
                        };

                        write!(
                            &mut msg,
                            "MESSAGE {} Resolved {} END",
                            config.instance_name,
                            info.get_fullname()
                        )?;
                        info!("ServiceResolved: sending message");
                        tx.send(msg).await?;
                    }
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

        // List all of the machine's network interfaces
        for iface in if_addrs::get_if_addrs().unwrap() {
            info!(
                "{:#?} {} {}",
                iface,
                if iface.is_loopback() { "LOOPBACK" } else { "" },
                if iface.is_link_local() { "LOCAL" } else { "" },
            );
        }

        info!("Spinning up UDP listener");
        let (udp_service, tx) = UdpService::new(&config).await?;

        info!("Spinning up mDNS discovery");
        let discovery_service = DiscoveryService::new(&config, udp_service.sock.local_addr()?);

        smol::future::try_zip(udp_service.run(), discovery_service.run(tx)).await?;

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
