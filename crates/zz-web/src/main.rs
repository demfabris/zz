use std::{net::SocketAddr, path::PathBuf};

use zz_web::{Gateway, GatewayConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bind: SocketAddr = "127.0.0.1:8080".parse()?;
    let mut assets = PathBuf::from("clients/web/dist");
    let mut socket = zz_daemon::default_socket_path();
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--bind") => {
                bind = arguments
                    .next()
                    .ok_or("--bind needs a loopback address and port")?
                    .to_str()
                    .ok_or("--bind must be valid UTF-8")?
                    .parse()?;
            }
            Some("--assets") => {
                assets = arguments.next().ok_or("--assets needs a directory")?.into();
            }
            Some("--socket") => {
                socket = arguments.next().ok_or("--socket needs a path")?.into();
            }
            Some("--help" | "-h") => {
                println!(
                    "Usage: zz-web [--assets clients/web/dist] [--bind 127.0.0.1:8080] [--socket PATH]\n\nServe the zz browser client and connect each browser to the existing daemon.\nOnly loopback addresses are supported. Use SSH forwarding for remote hosts."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {}", argument.to_string_lossy()).into()),
        }
    }
    let gateway = Gateway::bind(GatewayConfig {
        bind,
        assets,
        socket,
    })
    .await?;
    println!("zz browser client: http://{}", gateway.local_addr());
    gateway.serve().await?;
    Ok(())
}
