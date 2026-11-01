//! One Recruiter per database: a second start on the same database opens the one that runs
//! instead of failing on its port. The running app says which database it runs on at
//! GET instance; a start asks that first (running_on). The iced window will ask the running one
//! to come to the front the same way.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::server::AppState;

/// Where the running app answers which database it runs on.
pub const PATH: &str = "/instance";

/// What instance answers as app, so another program on the port isn't taken for Recruiter.
const APP: &str = "recruiter";

/// How long a start waits for a Recruiter on its port to answer: it answers in milliseconds, and
/// nothing on the port answers at once.
const ASK_TIMEOUT: Duration = Duration::from_secs(2);

/// GET instance's answer.
#[derive(Debug, Serialize, Deserialize)]
pub struct Instance {
    pub app: String,
    /// The database's full path, as the setup wizard shows it.
    pub database: String,
}

/// What runs on the port a start wants.
#[derive(Debug, PartialEq)]
pub enum Running {
    /// No Recruiter: nothing, or another program (which the start's own bind then says).
    None,
    /// A Recruiter on this database.
    Same,
    /// A Recruiter on another database, at this path.
    Other { database: String },
}

/// GET instance: this app, and the database it runs on.
pub async fn answer(State(state): State<Arc<AppState>>) -> Json<Instance> {
    Json(Instance {
        app: APP.to_string(),
        database: state.context.database.path.clone(),
    })
}

/// Whether a Recruiter answers on 127.0.0.1:port, and on which database; database is this
/// start's, as DatabaseFile::path shows it.
pub async fn running_on(port: u16, database: &str) -> Running {
    let Ok(client) = reqwest::Client::builder().no_proxy().timeout(ASK_TIMEOUT).build() else {
        return Running::None;
    };
    let url = format!("http://127.0.0.1:{port}{PATH}");
    let answer = match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => response.json::<Instance>().await.ok(),
        _ => None,
    };
    match answer {
        Some(instance) if instance.app == APP => compared(instance.database, database),
        _ => Running::None,
    }
}

/// Same when running is database, ignoring case on Windows, where paths do.
fn compared(running: String, database: &str) -> Running {
    let same = if cfg!(windows) {
        running.eq_ignore_ascii_case(database)
    } else {
        running == database
    };
    if same {
        Running::Same
    } else {
        Running::Other { database: running }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_database_is_the_same_path() {
        assert_eq!(
            compared("/home/a/recruiter.sqlite".to_string(), "/home/a/recruiter.sqlite"),
            Running::Same
        );
        assert_eq!(
            compared("/home/a/other.sqlite".to_string(), "/home/a/recruiter.sqlite"),
            Running::Other {
                database: "/home/a/other.sqlite".to_string()
            }
        );
    }

    #[tokio::test]
    async fn nothing_on_the_port_is_no_recruiter() {
        // Port 1 (tcpmux) is privileged and unused on a desktop.
        assert_eq!(running_on(1, "x").await, Running::None);
    }
}
