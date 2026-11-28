use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Router};
use clap::Parser;
use ognibuild::debian::apt::{dependency_to_possible_deb_dependencies, AptManager};
use ognibuild::debian::dep_server::{FamilyUnknown, ResolveAptRequest};
use ognibuild::dependency::dependency_from_json;
use ognibuild::session::{Session, SessionKind};
use std::io::Write;
use std::sync::mpsc;
use tokio::sync::oneshot;

type Reply = oneshot::Sender<Result<Option<Vec<String>>, String>>;

async fn resolve_apt(
    State(queue): State<mpsc::Sender<(String, serde_json::Value, Reply)>>,
    Json(request): Json<ResolveAptRequest>,
) -> Response {
    let (family, details) = request.requirement;
    let (reply, result) = oneshot::channel();
    queue.send((family.clone(), details, reply)).unwrap();
    match result.await.unwrap() {
        Ok(Some(deps)) => Json(deps).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            [("Reason", "family-unknown")],
            Json(FamilyUnknown {
                family,
                reason: "family-unknown".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

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

fn main() -> Result<(), i8> {
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

    let apt_mgr = AptManager::from_session(session.as_ref());
    let (queue, jobs) = mpsc::channel();

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/version", get(|| async { env!("CARGO_PKG_VERSION") }))
        .route("/ready", get(|| async { "ok" }))
        .route("/resolve-apt", post(resolve_apt))
        .with_state(queue);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let listener = rt
        .block_on(tokio::net::TcpListener::bind((
            args.listen_address.as_str(),
            args.port,
        )))
        .unwrap();
    log::info!("listening on {}", listener.local_addr().unwrap());
    let server = rt.spawn(async move { axum::serve(listener, app).await.unwrap() });
    // The session cannot leave this thread, so requests are resolved here, outside the runtime.
    for (family, details, reply) in jobs {
        let deps = dependency_from_json(&family, details).map(|dep| {
            dep.map(|dep| {
                dependency_to_possible_deb_dependencies(&apt_mgr, dep.as_ref())
                    .iter()
                    .map(|dep| dep.relation_string())
                    .collect()
            })
        });
        // The client may have gone away; nothing is waiting for the answer then.
        let _ = reply.send(deps.map_err(|e| e.to_string()));
    }
    rt.block_on(server).unwrap();
    Ok(())
}
