//! setup: the first-run wizard (GET setup), which the app opens on a database that hasn't been
//! set up (setup_steps::is_first_run). Its steps, in order:
//!
//! 1. The database: made at start-up; the wizard says where it is.
//! 2. The AI model: this app's own llama.cpp model when the computer has room for a good one,
//!    else an OpenAI or Claude key (model_advice). The steps themselves are the llama_setup and
//!    account pages, which come back here while the setup isn't finished.
//! 3. The CV, read into the profile (profile_fill), which needs the model.
//! 4. How often the job scan runs by itself: a Task Scheduler task (scan_schedule).
//! 5. The dashboard: the match page.
//!
//! The steps' state comes from setup_steps, so another front end can show the same steps.

pub mod setup_steps;

use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use self::setup_steps::{AiStep, CvStep, DatabaseStep, ScheduleStep};
use crate::account::model_advice::ModelAdvice;
use crate::profile::profile_fill::{self, MAX_CV_LENGTH};
use crate::scan_schedule::{DEFAULT_INTERVAL_HOURS, INTERVALS};
use crate::server::AppState;

pub use self::setup_steps::{first_run_or_logged, is_first_run};

/// Where the wizard is, and where its forms send the browser back to.
pub const PAGE: &str = "/setup";

/// Where the wizard ends: the match page, the app's dashboard.
const DASHBOARD: &str = "/match";

/// An interval in the schedule step's list.
pub struct IntervalOption {
    pub hours: u32,
    pub label: &'static str,
    pub selected: bool,
}

/// The CV step as the page prints it.
pub struct CvView {
    /// "Mette Sørensen, read from cv.pdf on 31 October 2026", when there is a profile.
    pub done: Option<String>,
    pub can_read: bool,
    /// Why the CV posted couldn't be read, with the text as it was posted.
    pub error: Option<String>,
    pub cv_text: String,
    pub cv_name: String,
    pub max_length: usize,
}

/// The schedule step as the page prints it.
pub struct ScheduleView {
    /// "every 4 hours", when the task is registered.
    pub done: Option<String>,
    pub error: Option<String>,
    pub intervals: Vec<IntervalOption>,
    pub task_run: String,
}

#[derive(Template)]
#[template(path = "setup/setup.html")]
struct SetupPage {
    flash: Option<String>,
    database: DatabaseStep,
    /// The saved keys, as "OpenAI, gpt-5.4-mini".
    ai_saved: Vec<String>,
    advice: ModelAdvice,
    cv: CvView,
    schedule: ScheduleView,
}

/// The CV form's fields.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct CvForm {
    cv_text: String,
    cv_name: String,
    /// "1" when replacing a profile there is already.
    replace: String,
}

/// The schedule form's field.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ScheduleForm {
    hours: u32,
}

/// GET /: the dashboard, which says what is set up and what is missing, with links here.
pub async fn home() -> Redirect {
    Redirect::to(DASHBOARD)
}

/// GET setup.
pub async fn index(State(state): State<Arc<AppState>>) -> Response {
    page(&state, None).await
}

/// POST setup/submit_cv {cv_text, cv_name, replace}: reads the CV into the profile, replacing
/// it; replace must be "1" when there is one. With an error the form comes back with the text.
pub async fn submit_cv(State(state): State<Arc<AppState>>, Form(form): Form<CvForm>) -> Response {
    let has_profile = match setup_steps::cv(&state).await {
        Ok(step) => step.filled.is_some(),
        Err(why) => return failure(&why),
    };
    let filled = if has_profile && form.replace != "1" {
        Err("Tick \"Replace my profile\" to fill it in again from this CV.".to_string())
    } else {
        profile_fill::fill_from_cv(&state, &form.cv_text, &form.cv_name).await
    };
    match filled {
        Ok(message) => {
            state.set_flash(message);
            Redirect::to(PAGE).into_response()
        }
        Err(why) => page(&state, Some((why, form))).await,
    }
}

/// POST setup/submit_schedule {hours}: registers the job scan's task to run every hours.
pub async fn submit_schedule(State(state): State<Arc<AppState>>, Form(form): Form<ScheduleForm>) -> Response {
    let message = match state.scan_schedule.register(form.hours).await {
        Ok(()) => format!(
            "The job scan runs {} while you're logged in to this computer.",
            setup_steps::interval_label(form.hours).to_lowercase()
        ),
        Err(why) => format!("The job scan couldn't be scheduled: {why}"),
    };
    state.set_flash(message);
    Redirect::to(PAGE).into_response()
}

/// POST setup/submit_unschedule: removes the job scan's task.
pub async fn submit_unschedule(State(state): State<Arc<AppState>>) -> Response {
    let message = match state.scan_schedule.remove().await {
        Ok(()) => "The job scan no longer runs by itself.".to_string(),
        Err(why) => format!("The job scan's task couldn't be removed: {why}"),
    };
    state.set_flash(message);
    Redirect::to(PAGE).into_response()
}

/// The page, with the CV form as posted and why it couldn't be read, when it couldn't.
async fn page(state: &AppState, posted: Option<(String, CvForm)>) -> Response {
    let ai = match setup_steps::ai(state).await {
        Ok(ai) => ai,
        Err(e) => return failure(&format!("The database couldn't answer: {e}")),
    };
    let cv = match setup_steps::cv(state).await {
        Ok(cv) => cv,
        Err(why) => return failure(&why),
    };
    let AiStep { saved, advice } = ai;
    let page = SetupPage {
        flash: state.take_flash(),
        database: setup_steps::database(state),
        ai_saved: saved,
        advice,
        cv: cv_view(cv, posted),
        schedule: schedule_view(setup_steps::schedule(state).await),
    };
    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => failure(&format!("The page couldn't be printed: {e}")),
    }
}

fn cv_view(step: CvStep, posted: Option<(String, CvForm)>) -> CvView {
    let (error, form) = match posted {
        Some((why, form)) => (Some(why), form),
        None => (None, CvForm::default()),
    };
    CvView {
        done: step.filled.as_ref().map(setup_steps::filled_line),
        can_read: step.can_read,
        error,
        cv_text: form.cv_text,
        cv_name: form.cv_name,
        max_length: MAX_CV_LENGTH,
    }
}

fn schedule_view(step: ScheduleStep) -> ScheduleView {
    let picked = step.interval_hours.unwrap_or(DEFAULT_INTERVAL_HOURS);
    ScheduleView {
        done: step
            .interval_hours
            .map(|hours| setup_steps::interval_label(hours).to_lowercase()),
        error: step.error,
        intervals: INTERVALS
            .iter()
            .map(|&(hours, label)| IntervalOption {
                hours,
                label,
                selected: hours == picked,
            })
            .collect(),
        task_run: step.task_run,
    }
}

/// The page when it can't be shown: what went wrong, logged too.
fn failure(message: &str) -> Response {
    eprintln!("setup: {message}");
    (StatusCode::INTERNAL_SERVER_ERROR, message.to_string()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::profile_model::FilledFrom;

    #[test]
    fn the_cv_step_says_what_the_profile_was_read_from() {
        let filled = FilledFrom {
            name: "Mette Sørensen".to_string(),
            cv_name: "cv.pdf".to_string(),
            // 31 October 2026, 12:00 UTC.
            filled_at: Some(1_793_448_000),
        };
        assert_eq!(
            setup_steps::filled_line(&filled),
            "Mette Sørensen, read from cv.pdf on 31 October 2026"
        );
        let imported = FilledFrom {
            cv_name: String::new(),
            filled_at: None,
            ..filled
        };
        assert_eq!(setup_steps::filled_line(&imported), "Mette Sørensen");
    }

    #[test]
    fn the_schedule_picks_the_tasks_interval_or_the_default() {
        let step = |interval_hours| ScheduleStep {
            interval_hours,
            error: None,
            task_run: String::new(),
        };
        let selected = |view: ScheduleView| view.intervals.iter().find(|o| o.selected).map(|o| o.hours);
        assert_eq!(selected(schedule_view(step(None))), Some(DEFAULT_INTERVAL_HOURS));
        let registered = schedule_view(step(Some(8)));
        assert_eq!(registered.done.as_deref(), Some("every 8 hours"));
        assert_eq!(selected(registered), Some(8));
    }
}
