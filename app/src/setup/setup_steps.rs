//! The setup wizard's steps as data and its forms' work, outside the web pages, so another front end (a native UI
//! later) can show the same steps from the same functions: where the database is, which model
//! the AI runs on, the profile read from the CV, and the job scan's schedule.
//!
//! The first run is told from the data itself, not from a flag: a database without a profile and
//! without saved matches hasn't been set up. Jonas's imported database has both, so it never
//! counts as a first run.

use crate::account::account_model::{self, RESUME};
use crate::account::model_advice::ModelAdvice;
use crate::context::Context;
use crate::matches::llm_key;
use crate::profile::profile_fill;
use crate::profile::profile_model::{self, FilledFrom};
use crate::scan_schedule::INTERVALS;
use crate::service_error::{ErrorKind, ServiceError};

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
pub async fn is_first_run(context: &Context) -> sqlx::Result<bool> {
    let has_profile = profile_model::filled_from(context.db(), context.user_id)
        .await?
        .is_some();
    if has_profile {
        return Ok(false);
    }
    let has_matches = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM matches WHERE trongate_user_id = ?1) AS "has!: bool""#,
        context.user_id
    )
    .fetch_one(context.db())
    .await?;
    Ok(!has_matches)
}

/// is_first_run, or false (logged) when the database can't say: the pages that only link back
/// to the wizard then go on without the link.
pub async fn first_run_or_logged(context: &Context) -> bool {
    is_first_run(context).await.unwrap_or_else(|e| {
        eprintln!("setup: whether this is the first run couldn't be read: {e}");
        false
    })
}

/// "Mette Sørensen, read from cv.pdf on 31 October 2026"; without the parts it doesn't have.
pub fn filled_line(filled: &FilledFrom) -> String {
    let mut line = filled.name.clone();
    if !filled.cv_name.is_empty() {
        line.push_str(&format!(", read from {}", filled.cv_name));
    }
    let day = filled
        .filled_at
        .and_then(|at| chrono::DateTime::from_timestamp(at, 0))
        .map(|at| at.format("%-d %B %Y").to_string());
    if let Some(day) = day {
        line.push_str(&format!(" on {day}"));
    }
    line
}

/// "Every 4 hours" for 4; "Every N hours" for one that isn't offered (set by hand).
pub fn interval_label(hours: u32) -> &'static str {
    INTERVALS
        .iter()
        .find(|(offered, _)| *offered == hours)
        .map(|(_, label)| *label)
        .unwrap_or("Every few hours")
}

pub fn database(context: &Context) -> DatabaseStep {
    DatabaseStep {
        path: context.database.path.clone(),
        made_now: context.database.made_now,
    }
}

pub async fn ai(context: &Context) -> sqlx::Result<AiStep> {
    let saved = account_model::saved(context.db(), context.user_id).await?;
    Ok(AiStep {
        saved: saved.iter().map(saved_key_line).collect(),
        advice: ModelAdvice::from(context.llama.as_ref()),
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

pub async fn cv(context: &Context) -> Result<CvStep, String> {
    let filled = profile_model::filled_from(context.db(), context.user_id)
        .await
        .map_err(|e| format!("The database couldn't answer: {e}"))?;
    let can_read = llm_key::llm_settings(context, RESUME)
        .await
        .map_err(|e| e.message)?
        .is_some();
    Ok(CvStep { filled, can_read })
}

pub async fn schedule(context: &Context) -> ScheduleStep {
    let (interval_hours, error) = match context.scan_schedule.interval_hours().await {
        Ok(interval) => (interval, None),
        Err(e) => (None, Some(e)),
    };
    ScheduleStep {
        interval_hours,
        error,
        task_run: context.scan_schedule.task_run(),
    }
}

/// Reads the CV into the profile, replacing it; replace must be true when there is a profile
/// already. Ok is what to say.
pub async fn read_cv(context: &Context, cv_text: &str, cv_name: &str, replace: bool) -> Result<String, ServiceError> {
    let has_profile = cv(context)
        .await
        .map_err(|why| ServiceError::internal("setup/read_cv", why))?
        .filled
        .is_some();
    if has_profile && !replace {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            "Tick \"Replace my profile\" to fill it in again from this CV.",
        ));
    }
    profile_fill::fill_from_cv(context, cv_text, cv_name).await
}

/// Registers the job scan's task to run every hours. Ok is what to say.
pub async fn schedule_scan(context: &Context, hours: u32) -> Result<String, ServiceError> {
    context.scan_schedule.register(hours).await.map_err(|why| {
        ServiceError::failed(
            "setup/schedule_scan",
            format!("The job scan couldn't be scheduled: {why}"),
        )
    })?;
    Ok(format!(
        "The job scan runs {} while you're logged in to this computer.",
        interval_label(hours).to_lowercase()
    ))
}

/// Removes the job scan's task. Ok is what to say.
pub async fn unschedule_scan(context: &Context) -> Result<&'static str, ServiceError> {
    context.scan_schedule.remove().await.map_err(|why| {
        ServiceError::failed(
            "setup/unschedule_scan",
            format!("The job scan's task couldn't be removed: {why}"),
        )
    })?;
    Ok("The job scan no longer runs by itself.")
}
