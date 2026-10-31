//! Reloading the page in the browser while developing (`recruiter --dev`, which app/dev.ps1
//! starts under watchexec).
//!
//! watchexec restarts the app whenever a Rust file, a template or an asset changes. Every page the
//! app sends then carries a small script that asks `/dev/started` which run of the app it is
//! talking to; when the answer changes, the app was restarted and the script reloads the page.
//! Only debug builds take `--dev`: release builds have the assets built in and never send the
//! script.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::server::AppState;

/// Where the page's script asks which run of the app it is talking to.
pub const STARTED_PATH: &str = "/dev/started";

/// How often the page's script asks, in milliseconds: fast enough to feel instant after a
/// restart, and only ever on this computer.
const POLL_MILLISECONDS: u32 = 500;

/// The most of a page this reads to add the script to, in bytes: well above the largest page,
/// so a page is never cut short.
const MAX_PAGE_BYTES: usize = 64 * 1024 * 1024;

/// What marks a whole page, which the script goes in front of; MX's partial answers have none
/// and are left as they are.
const PAGE_END: &str = "</body>";

/// One run of the app while developing.
pub struct DevReload {
    /// Names this run, different on every start: the process ID and the start time.
    started: String,
}

impl DevReload {
    pub fn new() -> Self {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        DevReload {
            started: format!("{}-{since_epoch}", std::process::id()),
        }
    }

    /// The script each page carries: it reloads the page when `/dev/started` names another run,
    /// and keeps asking while the app is down.
    fn script(&self) -> String {
        format!(
            r#"<script>
(() => {{
  const started = "{started}";
  const check = async () => {{
    try {{
      const answer = await fetch("{STARTED_PATH}", {{ cache: "no-store" }});
      if (answer.ok && (await answer.text()) !== started) {{
        location.reload();
        return;
      }}
    }} catch (_) {{}}
    setTimeout(check, {POLL_MILLISECONDS});
  }};
  setTimeout(check, {POLL_MILLISECONDS});
}})();
</script>
"#,
            started = self.started
        )
    }

    /// The page with the script put in front of its last `</body>`, or None when it isn't a
    /// whole page.
    fn with_script(&self, page: &str) -> Option<String> {
        let end = page.rfind(PAGE_END)?;
        Some(format!("{}{}{}", &page[..end], self.script(), &page[end..]))
    }
}

impl Default for DevReload {
    fn default() -> Self {
        Self::new()
    }
}

/// Answers which run of the app this is.
pub async fn started(State(state): State<Arc<AppState>>) -> Response {
    match &state.dev_reload {
        Some(dev_reload) => dev_reload.started.clone().into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

/// Adds the reload script to every whole HTML page, when the app runs with `--dev`.
pub async fn add_script(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let Some(dev_reload) = &state.dev_reload else {
        return response;
    };
    if !is_html(&response) {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, MAX_PAGE_BYTES).await {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("Couldn't read a page to add the reload script: {e}");
            return (parts.status, "The page couldn't be read.").into_response();
        }
    };
    let Some(page) = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|page| dev_reload.with_script(page))
    else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(page))
}

fn is_html(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_goes_in_front_of_the_last_body_end() {
        let dev_reload = DevReload::new();
        let page = dev_reload.with_script("<html><body>x</body></html>").unwrap();
        assert!(page.starts_with("<html><body>x<script>"));
        assert!(page.ends_with("</script>\n</body></html>"));
        assert!(page.contains(&dev_reload.started));
    }

    #[test]
    fn a_partial_answer_gets_no_script() {
        assert_eq!(DevReload::new().with_script("<div>x</div>"), None);
    }
}
