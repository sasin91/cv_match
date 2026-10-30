//! The app's local web server: the match page and its endpoints on 127.0.0.1, with the page's
//! CSS and JS from modules/cv_match and Trongate MX from public/js, the same files the PHP app
//! serves.
//!
//! Only this computer can reach it, but any web page open in a browser here could send it
//! requests, so it answers a request only when its Host header names this server (no DNS
//! rebinding) and, for anything but GET, when its Origin is this server's own page or the Jobnet
//! extension (extension::ID).

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use rusqlite::Connection;
use rust_embed::RustEmbed;

use crate::cv_match::pages;
use crate::extension;
use crate::taxonomy::Taxonomy;

/// The port the app listens on unless told otherwise, fixed so the Jobnet extension can find it.
pub const DEFAULT_PORT: u16 = 8765;

/// modules/cv_match's css/ and js/, served under cv_match_module/ as Trongate serves them.
#[derive(RustEmbed)]
#[folder = "../../modules/cv_match/"]
#[include = "css/*"]
#[include = "js/*"]
struct CvMatchAssets;

/// Trongate MX, served under js/.
#[derive(RustEmbed)]
#[folder = "../../public/js/"]
#[include = "trongate-mx.min.js"]
struct PublicJs;

/// What every request shares: the database, the taxonomy, the local user and the message the
/// next page shows (Trongate's flashdata).
pub struct AppState {
    db: Mutex<Connection>,
    pub taxonomy: Arc<Taxonomy>,
    pub user_id: i64,
    flash: Mutex<Option<String>>,
    port: u16,
    /// Where the Jobnet extension was written for the browser to load, or why it couldn't be.
    pub extension_folder: Result<PathBuf, String>,
}

impl AppState {
    pub fn new(db: Connection, taxonomy: Arc<Taxonomy>, user_id: i64, port: u16) -> Self {
        AppState {
            db: Mutex::new(db),
            taxonomy,
            user_id,
            flash: Mutex::new(None),
            port,
            extension_folder: Err("The extension wasn't written out.".to_string()),
        }
    }

    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_flash(&self, message: String) {
        *self.flash.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(message);
    }

    pub fn take_flash(&self) -> Option<String> {
        self.flash
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }
}

/// The local user: the one imported, or a new one on a fresh database.
pub fn local_user(db: &Connection) -> rusqlite::Result<i64> {
    db.execute(
        "INSERT INTO trongate_users (id) SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM trongate_users)",
        [],
    )?;
    db.query_row("SELECT MIN(id) FROM trongate_users", [], |row| row.get(0))
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(pages::home))
        .route("/cv_match", get(pages::index))
        .route("/cv_match/{id}", get(pages::show))
        .route("/cv_match/history_list", get(pages::history_list))
        .route("/cv_match/history_more", get(pages::history_more))
        .route("/cv_match/result/{id}", get(pages::result))
        .route("/cv_match/quick_match", post(pages::quick_match))
        .route("/cv_match/already_matched", post(pages::already_matched))
        .route("/cv_match/submit_reject/{id}", post(pages::submit_reject))
        .route("/cv_match/history", get(pages::history))
        .route("/cv_match/joblog/{id}", get(pages::joblog))
        .route("/cv_match/joblog_panel/{id}", get(pages::joblog_panel))
        .route("/cv_match/joblog_status", post(pages::joblog_status))
        .route("/joblog_extension", get(pages::joblog_extension))
        .route("/cv_match_module/{*path}", get(cv_match_asset))
        .route("/js/{*path}", get(public_js))
        .fallback(pages::not_yet)
        .method_not_allowed_fallback(pages::not_yet)
        .layer(middleware::from_fn_with_state(state.clone(), only_this_page))
        .with_state(state)
}

/// Answers on 127.0.0.1:port until the process ends.
pub async fn serve(state: Arc<AppState>) -> std::io::Result<()> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, state.port));
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(state)).await
}

/// The addresses this server's pages come from.
fn own_hosts(port: u16) -> [String; 2] {
    [format!("127.0.0.1:{port}"), format!("localhost:{port}")]
}

/// Lets a request through only when it is for this server and, unless it is a GET, from its own
/// page.
async fn only_this_page(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    if let Err(why) = allowed(request.method(), request.headers(), state.port) {
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    next.run(request).await
}

fn allowed(method: &Method, headers: &HeaderMap, port: u16) -> Result<(), &'static str> {
    let hosts = own_hosts(port);
    let host = headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .unwrap_or("");
    if !hosts.iter().any(|own| own == host) {
        return Err("This is the Recruiter app on this computer; open it at its own address.");
    }
    if method == Method::GET || method == Method::HEAD {
        return Ok(());
    }
    let origin = headers.get(header::ORIGIN).and_then(|origin| origin.to_str().ok());
    match origin {
        Some(origin) if hosts.iter().any(|own| origin == format!("http://{own}")) => Ok(()),
        Some(origin) if origin == format!("chrome-extension://{}", extension::ID) => Ok(()),
        _ => Err("Only the Recruiter app's own pages and its Jobnet extension can send it changes."),
    }
}

async fn cv_match_asset(Path(path): Path<String>) -> Response {
    asset(CvMatchAssets::get(&path), &path)
}

async fn public_js(Path(path): Path<String>) -> Response {
    asset(PublicJs::get(&path), &path)
}

fn asset(file: Option<rust_embed::EmbeddedFile>, path: &str) -> Response {
    let Some(file) = file else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(file.data.into_owned()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
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
    fn only_this_server_and_its_own_page_get_through() {
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
