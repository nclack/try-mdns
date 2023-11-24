use std::net::{IpAddr, SocketAddr};

use anyhow::Result;
use clap::Parser;
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
    /// Ip Address
    address: IpAddr,

    /// Port
    #[arg(short, default_value = "0")]
    port: u16,
}

fn main() -> Result<()> {
    dotenv::dotenv().ok();
    pretty_env_logger::init();
    let config = Config::parse();

    smol::block_on(async {
        let dst = SocketAddr::from((config.address, config.port));
        let socket = UdpSocket::bind(("127.0.0.1", 0)).await?;
        let s = "Hello There";
        socket.send_to(s.as_bytes(), dst).await?;
        Ok(())
    })
}
