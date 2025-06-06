use axum::{routing::get, Router};
use clap::Parser;
use ognibuild::debian::apt::AptManager;
#[cfg(target_os = "linux")]
use ognibuild::session::schroot::SchrootSession;
use ognibuild::session::{plain::PlainSession, Session};
use std::io::Write;

#[derive(Parser)]
struct Args {
    #[clap(short, long)]
    listen_address: String,

    #[clap(short, long)]
    port: u16,

    #[cfg(target_os = "linux")]
    #[clap(short, long)]
    schroot: Option<String>,

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
    let session: Box<dyn Session> = if let Some(schroot) = args.schroot {
        Box::new(SchrootSession::new(&schroot, None).unwrap())
    } else {
        Box::new(PlainSession::new())
    };

    #[cfg(not(target_os = "linux"))]
    let session: Box<dyn Session> = Box::new(PlainSession::new());

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
