//! The setup wizard's steps as data, outside the web pages, so another front end (a native UI
//! later) can show the same steps from the same functions: where the database is, which model
//! the AI runs on, the profile read from the CV, and the job scan's schedule.
//!
//! The first run is told from the data itself, not from a flag: a database without a profile and
//! without saved matches hasn't been set up. Jonas's imported database has both, so it never
//! counts as a first run.

use crate::account::account_model::{self, RESUME};
use crate::account::model_advice::ModelAdvice;
use crate::matches::llm_key;
use crate::profile::profile_model::{self, FilledFrom};
use crate::server::AppState;

/// The database step: the file the app keeps everything in.
#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseStep {
    pub path: String,
    /// Made by this start of the app, rather than found.
    pub made_now: bool,
}

/// The AI step: the saved keys, and the suggestion while there are none.
pub struct AiStep {
    /// Each saved key as "This app's own model (llama.cpp), Qwen3-8B-Q4_K_M.gguf".
    pub saved: Vec<String>,
    pub advice: ModelAdvice,
}

/// The CV step: what the profile was filled from, and whether a CV can be read now.
pub struct CvStep {
    pub filled: Option<FilledFrom>,
    /// A key is picked for tailoring résumés, which reading the CV runs on.
    pub can_read: bool,
}

/// The schedule step: how often the job scan runs by itself, if it does.
pub struct ScheduleStep {
    /// The task's interval in hours, None when there is no task.
    pub interval_hours: Option<u32>,
    /// Why Task Scheduler couldn't be read, when it couldn't.
    pub error: Option<String>,
    /// The command the task runs.
    pub task_run: String,
}

/// Whether the app hasn't been set up yet: no profile and no saved matches.
pub async fn is_first_run(state: &AppState) -> sqlx::Result<bool> {
    let has_profile = profile_model::filled_from(state.db(), state.user_id).await?.is_some();
    if has_profile {
        return Ok(false);
    }
    let has_matches = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM matches WHERE trongate_user_id = ?1) AS "has!: bool""#,
        state.user_id
    )
    .fetch_one(state.db())
    .await?;
    Ok(!has_matches)
}

/// is_first_run, or false (logged) when the database can't say: the pages that only link back
/// to the wizard then go on without the link.
pub async fn first_run_or_logged(state: &AppState) -> bool {
    is_first_run(state).await.unwrap_or_else(|e| {
        eprintln!("setup: whether this is the first run couldn't be read: {e}");
        false
    })
}

pub fn database(state: &AppState) -> DatabaseStep {
    DatabaseStep {
        path: state.database.path.clone(),
        made_now: state.database.made_now,
    }
}

pub async fn ai(state: &AppState) -> sqlx::Result<AiStep> {
    let saved = account_model::saved(state.db(), state.user_id).await?;
    Ok(AiStep {
        saved: saved.iter().map(saved_key_line).collect(),
        advice: ModelAdvice::from(state.llama.as_ref()),
    })
}

/// "OpenAI, gpt-5.4-mini", or the provider alone without a model.
fn saved_key_line(saved: &account_model::Saved) -> String {
    let provider = account_model::provider_label(&saved.provider);
    if saved.model.is_empty() {
        provider.to_string()
    } else {
        format!("{provider}, {}", saved.model)
    }
}

pub async fn cv(state: &AppState) -> Result<CvStep, String> {
    let filled = profile_model::filled_from(state.db(), state.user_id)
        .await
        .map_err(|e| format!("The database couldn't answer: {e}"))?;
    let can_read = llm_key::llm_settings(state, RESUME)
        .await
        .map_err(|e| e.message)?
        .is_some();
    Ok(CvStep { filled, can_read })
}

pub async fn schedule(state: &AppState) -> ScheduleStep {
    let (interval_hours, error) = match state.scan_schedule.interval_hours().await {
        Ok(interval) => (interval, None),
        Err(e) => (None, Some(e)),
    };
    ScheduleStep {
        interval_hours,
        error,
        task_run: state.scan_schedule.task_run(),
    }
}
