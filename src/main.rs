use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use log::info;
use mdns_sd::ServiceInfo;

#[derive(Parser, Debug)]
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
    #[arg(short, default_value = "8090")]
    port: u16,

    /// Key=Value properties to share with peers.
    #[arg(value_parser=parse_key_val::<String,String>)]
    properties: Vec<(String, String)>,
}

#[pollster::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    pretty_env_logger::init();
    let config = Config::parse();

    info!("Hi there! {config:?}");

    let service_name = format!("{}.local.", config.service_name);

    let info = ServiceInfo::new(
        &service_name,
        &config.instance_name,
        gethostname::gethostname().to_str().unwrap(),
        "",
        config.port,
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
        futures_timer::Delay::new(Duration::from_millis(500)).await;
    }
    Ok(())
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
