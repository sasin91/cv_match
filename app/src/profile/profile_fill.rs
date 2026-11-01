//! Filling the profile in from a CV, ported from modules/profile/Profile.php (submit_fill and
//! read_cv): one model call reads the CV's text into the profile, replacing all of it. It runs
//! on the key picked for tailoring résumés, since the résumés are written from the profile.

use crate::account::account_model::RESUME;
use crate::context::Context;
use crate::llm::{Effort, Llm, LlmException};
use crate::matches::llm_key;
use crate::service_error::{ErrorKind, ServiceError};

use super::{profile_fields, profile_model};

/// Shortest CV text read, in characters: anything shorter is not a whole CV.
pub const MIN_CV_LENGTH: usize = 100;
/// Longest CV text read, in characters.
pub const MAX_CV_LENGTH: usize = 60_000;

/// What the model is told to do with the CV (Profile::read_cv), with email added.
const READ_CV_SYSTEM: &str = r#"You read a candidate's CV into their profile, which their job applications and résumés will be written from. Copy what the CV says; never add, improve or guess anything.

- name, email and phone: the candidate's, as the CV gives them; empty when it doesn't.
- title: their professional title as the CV gives it; location: where they live, as written ("Slagelse, 4200").
- about: the CV's own profile or summary paragraphs, word for word; empty when it has none.
- entries: every job (section experience), every project or product of the candidate's own, side projects and open source included (section project), and every education (section education), in the CV's order.
  - title: the job title, the project's name or the education; organisation: the employer or school, empty for an own project; location as written; url: a project's link if the CV gives one.
  - started and ended as "YYYY-MM" (a year alone as "YYYY"); ended empty when it is ongoing ("now", "present", "nu").
  - summary: the line about what the employer or project is ("Job & candidate matchmaking platform"); bullets: the CV's bullets for it, word for word; note: context about it, like why a role ended.
- skills, languages and certificates: as the CV lists them, one item each.
Keep the CV's language."#;

/// The CV as the model reads it, before profile_fields::from_cv cleans it.
async fn read_cv(llm: &Llm, cv: &str) -> Result<serde_json::Value, LlmException> {
    llm.structured(
        READ_CV_SYSTEM,
        &format!("CV:\n\n{cv}"),
        &profile_fields::schema(),
        Effort::Low,
    )
    .await
}

/// Why the CV text can't be read, in Trongate validation's words; None when it can.
pub fn cv_text_error(cv_text: &str) -> Option<String> {
    let length = cv_text.trim().chars().count();
    if length == 0 {
        return Some("The CV field is required.".to_string());
    }
    if length < MIN_CV_LENGTH {
        return Some(format!(
            "The CV field must be at least {MIN_CV_LENGTH} characters in length. Check that the whole CV was pasted."
        ));
    }
    if length > MAX_CV_LENGTH {
        return Some(format!(
            "The CV field cannot exceed {MAX_CV_LENGTH} characters in length."
        ));
    }
    None
}

/// Reads the CV into the user's profile, replacing it: Ok with what to tell the user, or Err
/// with why not (logged).
pub async fn fill_from_cv(context: &Context, cv_text: &str, cv_name: &str) -> Result<String, ServiceError> {
    if let Some(why) = cv_text_error(cv_text) {
        return Err(ServiceError::new(ErrorKind::Invalid, why));
    }
    let keyed = llm_key::llm(context, RESUME).await?;
    let answer = read_cv(&keyed.llm, cv_text.trim()).await.map_err(|e| {
        crate::log_line!("profile (fill_from_cv): {}", e.details());
        ServiceError::new(
            ErrorKind::Llm { retryable: e.retryable },
            format!("The CV could not be read into your profile: {e}"),
        )
    })?;
    let profile = profile_fields::from_cv(&answer).ok_or_else(|| {
        crate::log_line!("profile (fill_from_cv): no entries in the answer: {answer}");
        ServiceError::new(
            ErrorKind::Failed,
            "No jobs, projects or education were found in that CV. Check that the whole CV was pasted and try again.",
        )
    })?;
    profile_model::replace_from_cv(context.db(), context.user_id, &profile, cv_name)
        .await
        .map_err(|e| ServiceError::internal("profile (fill_from_cv)", format!("The profile couldn't be saved: {e}")))?;
    Ok(format!(
        "Your profile is filled in from the CV: {} jobs, projects and educations. The match page matches with it.",
        profile.entries.len()
    ))
}
