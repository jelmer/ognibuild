use axum::{routing::get, Router};
use clap::Parser;
use ognibuild::debian::apt::AptManager;
use ognibuild::session::{Session, SessionKind};
use std::io::Write;

#[derive(Parser)]
struct Args {
    #[clap(short, long, required = true)]
    /// Address to listen on, repeatable; a host name means every address it resolves to, :: means IPv6 only
    listen_address: Vec<String>,

    #[clap(short, long)]
    port: u16,

    #[cfg(target_os = "linux")]
    #[clap(short, long)]
    /// schroot chroot to run in (shorthand for --session schroot:<name>)
    schroot: Option<String>,

    #[clap(long)]
    /// Session backend to run in: "plain", "schroot:<name>", or "unshare:<suite>"
    session: Option<SessionKind>,

    #[clap(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<(), i8> {
    let args = Args::parse();

    env_logger::builder()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter(
            None,
            if args.debug {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            },
        )
        .init();

    #[cfg(target_os = "linux")]
    let schroot = args.schroot;
    #[cfg(not(target_os = "linux"))]
    let schroot = None;
    let session_kind = ognibuild::session::resolve_session_kind(args.session, schroot)
        .unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });

    let session: Box<dyn Session> = session_kind.build(None).unwrap();

    let _apt_mgr = AptManager::from_session(session.as_ref());

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/version", get(|| async { env!("CARGO_PKG_VERSION") }))
        .route("/ready", get(|| async { "ok" }));

    let mut servers = tokio::task::JoinSet::new();
    for listener in bind_all(&args.listen_address, args.port).await.unwrap() {
        log::info!("listening on {}", listener.local_addr().unwrap());
        let app = app.clone();
        servers.spawn(async move { axum::serve(listener, app).await.unwrap() });
    }
    servers.join_all().await;
    Ok(())
}

/// Bind every address `hosts` resolve to; skip one this host does not have if another binds.
async fn bind_all(hosts: &[String], port: u16) -> std::io::Result<Vec<tokio::net::TcpListener>> {
    let mut listeners: Vec<tokio::net::TcpListener> = Vec::new();
    let mut unavailable = None;
    let mut seen = std::collections::HashSet::new();
    for host in hosts {
        let addrs = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {}", host, e)))?;
        for mut addr in addrs {
            addr.set_ip(addr.ip().to_canonical());
            if !seen.insert(addr) {
                continue;
            }
            if let (0, Some(first)) = (addr.port(), listeners.first()) {
                addr.set_port(first.local_addr()?.port());
            }
            match bind(addr) {
                Ok(listener) => listeners.push(listener),
                Err(e)
                    if e.kind() == std::io::ErrorKind::AddrNotAvailable
                        || e.raw_os_error() == Some(libc::EAFNOSUPPORT) =>
                {
                    log::warn!("not listening on {}: {}", addr, e);
                    unavailable = Some(std::io::Error::new(e.kind(), format!("{}: {}", addr, e)));
                }
                Err(e) => return Err(std::io::Error::new(e.kind(), format!("{}: {}", addr, e))),
            }
        }
    }
    if listeners.is_empty() {
        return Err(unavailable.unwrap_or_else(|| std::io::Error::other("no address to listen on")));
    }
    Ok(listeners)
}

/// Bind `addr`; an IPv6 socket takes IPv6 only, so `::` leaves IPv4 to `0.0.0.0`.
fn bind(addr: std::net::SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    let socket = if addr.is_ipv6() {
        let socket = tokio::net::TcpSocket::new_v6()?;
        socket2::SockRef::from(&socket).set_only_v6(true)?;
        socket
    } else {
        tokio::net::TcpSocket::new_v4()?
    };
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    socket.listen(1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bind_all() {
        if std::net::TcpListener::bind("[::1]:0").is_err()
            || std::net::TcpListener::bind("127.0.0.1:0").is_err()
        {
            return;
        }
        let hosts = ["::ffff:127.0.0.1", "127.0.0.1", "::"].map(String::from);
        let listeners = bind_all(&hosts, 0).await.unwrap();
        let addrs: Vec<_> = listeners.iter().map(|l| l.local_addr().unwrap()).collect();
        let port = addrs[0].port();
        assert_eq!(
            addrs,
            vec![
                std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                std::net::SocketAddr::from(([0u16; 8], port)),
            ]
        );
    }
}
