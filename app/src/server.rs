//! The app's local web server on 127.0.0.1: what the Jobnet extension asks for (extension_api)
//! and how a second start finds this one (instance). The app itself is its window (desktop);
//! there are no web pages.
//!
//! Only this computer can reach it, but any web page open in a browser here could send it
//! requests, so it answers a request only when its Host header names this server (no DNS
//! rebinding) and, for anything but GET, when its Origin is this server's own address (a second
//! start, instance::bring_to_front) or the Jobnet extension (extension::ID).

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};

use crate::context::Context;
use crate::extension;
use crate::extension_api;
use crate::instance;
use crate::job_scan;

/// The port the app listens on unless told otherwise, fixed so the Jobnet extension can find it.
pub const DEFAULT_PORT: u16 = 8765;

/// Bytes of JSON around a sent page's HTML (its link, its search, the escaping), at most.
const SENT_PAGE_JSON_ROOM: usize = 64 * 1024;

/// Most bytes of a page the extension sends (job_scan/submit_page): its HTML, and its JSON.
const SENT_PAGE_BYTES: usize = job_scan::scan_service::MAX_SENT_HTML + SENT_PAGE_JSON_ROOM;

/// What every request shares: the app's work (Context, which the desktop window shares too), the
/// port, and how to bring the window to the front.
pub struct AppState {
    pub context: Arc<Context>,
    port: u16,
    /// Tells the desktop window to come to the front (instance::front), once it is open.
    pub front: Option<tokio::sync::mpsc::UnboundedSender<()>>,
}

impl AppState {
    pub fn new(context: Arc<Context>, port: u16) -> Self {
        AppState {
            context,
            port,
            front: None,
        }
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(instance::PATH, get(instance::answer))
        .route(instance::FRONT_PATH, post(instance::front))
        .route("/match/history", get(extension_api::history))
        .route("/match/joblog/{id}", get(extension_api::joblog))
        .route("/match/joblog_status", post(extension_api::joblog_status))
        .route("/match/pdf", post(extension_api::pdf))
        .route("/job_scan/search_names", get(extension_api::search_names))
        .route(
            "/job_scan/submit_page",
            post(extension_api::submit_page).layer(DefaultBodyLimit::max(SENT_PAGE_BYTES)),
        )
        .fallback(extension_api::not_here)
        .method_not_allowed_fallback(extension_api::not_here)
        .layer(middleware::from_fn_with_state(state.clone(), only_this_app))
        .with_state(state)
}

/// The app's port on 127.0.0.1, taken before the window opens, so a taken port is said at start.
pub async fn bind(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await
}

/// Answers on listener (bind) until the process ends.
pub async fn serve_on(listener: tokio::net::TcpListener, state: Arc<AppState>) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}

/// The Host headers this server answers to: its own address.
fn own_hosts(port: u16) -> [String; 2] {
    [format!("127.0.0.1:{port}"), format!("localhost:{port}")]
}

/// The origins of this server's own address, as a request sends them in Origin.
fn own_origins(port: u16) -> [String; 2] {
    [format!("http://127.0.0.1:{port}"), format!("http://localhost:{port}")]
}

/// Lets a request through only when it is for this server and, unless it is a GET, from this app
/// or its extension. What it runs is in the task manager's list (its model calls, say).
async fn only_this_app(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    if let Err(why) = allowed(request.method(), request.headers(), state.port) {
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    let running = Arc::clone(&state.context.running);
    running.within(next.run(request)).await
}

fn allowed(method: &Method, headers: &HeaderMap, port: u16) -> Result<(), &'static str> {
    let host = headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .unwrap_or("");
    if !own_hosts(port).iter().any(|own| own == host) {
        return Err("This is the JobFinder app on this computer; it answers only at its own address.");
    }
    if method == Method::GET || method == Method::HEAD {
        return Ok(());
    }
    let origin = headers.get(header::ORIGIN).and_then(|origin| origin.to_str().ok());
    match origin {
        Some(origin) if own_origins(port).iter().any(|own| own == origin) => Ok(()),
        Some(origin) if origin == format!("chrome-extension://{}", extension::ID) => Ok(()),
        _ => Err("Only the JobFinder app and its Jobnet extension can send it changes."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(name.clone(), value.parse().unwrap());
        }
        map
    }

    #[test]
    fn only_this_server_this_app_and_its_extension_get_through() {
        let port = DEFAULT_PORT;
        let own = headers(&[(header::HOST, "127.0.0.1:8765")]);
        assert!(allowed(&Method::GET, &own, port).is_ok());
        // DNS rebinding: a site whose name points at 127.0.0.1.
        assert!(allowed(&Method::GET, &headers(&[(header::HOST, "evil.example:8765")]), port).is_err());
        // A form on another site posting here.
        let cross = headers(&[
            (header::HOST, "127.0.0.1:8765"),
            (header::ORIGIN, "https://evil.example"),
        ]);
        assert!(allowed(&Method::POST, &cross, port).is_err());
        assert!(allowed(&Method::POST, &own, port).is_err());
        let same = headers(&[
            (header::HOST, "localhost:8765"),
            (header::ORIGIN, "http://localhost:8765"),
        ]);
        assert!(allowed(&Method::POST, &same, port).is_ok());
        // The Jobnet extension (Edge and Chrome), and no other.
        let ours = headers(&[
            (header::HOST, "127.0.0.1:8765"),
            (header::ORIGIN, "chrome-extension://jijhihfppobcoconjjllmffbohpffbll"),
        ]);
        assert!(allowed(&Method::POST, &ours, port).is_ok());
        let other = headers(&[
            (header::HOST, "127.0.0.1:8765"),
            (header::ORIGIN, "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ]);
        assert!(allowed(&Method::POST, &other, port).is_err());
    }
}
