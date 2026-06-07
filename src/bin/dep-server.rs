use axum::{routing::get, Router};
use clap::Parser;
use ognibuild::debian::apt::AptManager;
use ognibuild::session::{Session, SessionKind};
use std::io::Write;

#[derive(Parser)]
struct Args {
    #[clap(short, long)]
    listen_address: String,

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

    let listener = tokio::net::TcpListener::bind((args.listen_address.as_str(), args.port))
        .await
        .unwrap();
    log::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
    Ok(())
}
