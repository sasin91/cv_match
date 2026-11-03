//! What a paired device does with the user's matches: reads a list of saved matches and one
//! match, as the computer's Match screen shows them, and sends a job post to be matched with the
//! user's profile (evaluate), which runs in the task manager like a match started in the window
//! and is saved like one. What became of each post a device sent is kept in
//! Devices::evaluations while the app runs.

use std::sync::Arc;

use device_protocol::{
    EvaluateAsk, Evaluation, Evaluations, ListName, MAX_EVALUATE_CHARS, MatchDetails, MatchRow, Matches, MatchesAsk,
    OpeningText, Requirement, RequirementGroup,
};

use super::device_model::Device;
use super::evaluations::{Outcome, WORKING_PER_DEVICE};
use crate::context::Context;
use crate::matches::match_model::{LISTS, SHORTLIST};
use crate::matches::match_result::MatchResult;
use crate::matches::match_service::{self, MatchAnswer, MatchAsk};
use crate::matches::verdict_row::VerdictRow;
use crate::profile;
use crate::service_error::{ErrorKind, ServiceError};
use crate::task_manager::running_work::WorkKind;

/// A batch of the list ask names (the shortlist when it names none it knows), best first.
pub async fn matches(context: &Context, ask: &MatchesAsk) -> Result<Matches, ServiceError> {
    let list = LISTS
        .iter()
        .map(|(name, ..)| *name)
        .find(|name| ask.list.as_deref() == Some(*name))
        .unwrap_or(SHORTLIST);
    let found = match_service::saved_matches(context, Some(list), &ask.after).await?;
    Ok(Matches {
        lists: LISTS
            .iter()
            .map(|(name, label, empty)| ListName {
                name: name.to_string(),
                label: label.to_string(),
                empty: empty.to_string(),
            })
            .collect(),
        list: list.to_string(),
        matches: found
            .matches
            .into_iter()
            .map(|row| MatchRow {
                id: row.id,
                tag: row.tag,
                title: row.title,
                about: row.about,
                saved_at: row.saved_at,
                saved_on: row.saved_on,
                done: row.done,
                opening: row.opening,
                closed: row.closed,
            })
            .collect(),
        next_after: found.next_after,
    })
}

/// The saved match with this id, as a device shows it.
pub async fn match_details(context: &Context, id: i64) -> Result<MatchDetails, ServiceError> {
    let result = match_service::saved_result(context, id).await?;
    Ok(details(id, &result))
}

fn details(id: i64, result: &MatchResult) -> MatchDetails {
    MatchDetails {
        id,
        percent: result.score.percent,
        tag: result.score.tag.clone(),
        tag_name: result.score.tag_name.clone(),
        heading: result.heading.clone(),
        parts: result.parts.clone(),
        saved_at: result.saved_at.clone(),
        job_url: result.job_url.clone(),
        opening: result.opening.as_ref().map(|opening| OpeningText {
            text: opening.text.clone(),
            closed: opening.closed,
        }),
        disqualifiers: result
            .disqualifier
            .iter()
            .flat_map(|notice| &notice.lines)
            .map(|line| Requirement {
                verdict: "missing".to_string(),
                text: line.text.clone(),
                reason: line.reason.clone(),
                evidence: String::new(),
                decided_by: String::new(),
                alternatives: Vec::new(),
            })
            .collect(),
        sections: result
            .sections
            .iter()
            .map(|section| RequirementGroup {
                heading: section.heading.clone(),
                note: section.note.clone(),
                rows: section.rows.iter().map(requirement).collect(),
            })
            .collect(),
    }
}

fn requirement(row: &VerdictRow) -> Requirement {
    Requirement {
        verdict: row.verdict.to_string(),
        text: row.text.clone(),
        reason: row.reason.clone(),
        evidence: row.evidence.clone(),
        decided_by: row.decided_by.clone(),
        alternatives: row.alternatives.iter().map(requirement).collect(),
    }
}

/// Starts matching the job post device sent (a link or its text) with the user's profile, in the
/// task manager, and answers the evaluation as it starts; the device asks for it again
/// (evaluations) until it is done.
pub fn evaluate(
    context: &Arc<Context>,
    device: &Device,
    ask: &EvaluateAsk,
    now: i64,
) -> Result<Evaluation, ServiceError> {
    let text = ask.text.trim().to_string();
    if text.is_empty() {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            "Send a link to a job post, or the post's text.",
        ));
    }
    if text.chars().count() > MAX_EVALUATE_CHARS {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            format!("That's longer than a job post: send at most {MAX_EVALUATE_CHARS} characters, or a link to it."),
        ));
    }
    let Some(evaluation) = context.devices.evaluations.start(device.id, &text, now) else {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            format!(
                "This device has {WORKING_PER_DEVICE} job posts being matched already. Send this one when one of them is done."
            ),
        ));
    };
    let (device_id, id) = (device.id, evaluation.id);
    let what = format!("From {}: {}", device.name, evaluation.sent);
    let task_context = Arc::clone(context);
    context.running.spawn(async move {
        let context = task_context;
        let work = context
            .running
            .run(WorkKind::Matching, what, evaluate_now(&context, device_id, id, &text));
        let outcome = match work.await {
            Ok(Ok(outcome)) => Ok(outcome),
            Ok(Err(e)) => {
                crate::log_line!(
                    "devices: a job post from device {device_id} couldn't be matched: {}",
                    e.message
                );
                Err(e.message)
            }
            Err(_stopped) => Err("Stopped from the task manager on the computer.".to_string()),
        };
        context.devices.evaluations.finish(device_id, id, outcome);
    });
    Ok(evaluation)
}

/// The job posts device sent while the app has run, the latest first.
pub fn evaluations(context: &Context, device: &Device) -> Evaluations {
    Evaluations {
        evaluations: context.devices.evaluations.of(device.id),
    }
}

/// Matches text with the user's profile as the Match screen would, saying each step on the
/// evaluation.
async fn evaluate_now(context: &Context, device_id: i64, id: u64, text: &str) -> Result<Outcome, ServiceError> {
    let evaluations = &context.devices.evaluations;
    let cv_text = profile::text_for_user(context.db())
        .await
        .map_err(|e| ServiceError::internal("devices/evaluate", format!("Your profile couldn't be read: {e}")))?
        .ok_or_else(|| {
            ServiceError::new(
                ErrorKind::Invalid,
                "There's no profile on the computer to match with yet: add your CV under Settings, Setup.",
            )
        })?;
    let read_cv = evaluations.read_cv();
    let ask = MatchAsk {
        job_text: text,
        job_url: "",
        cv_text: &cv_text,
        read_cv: read_cv.as_ref(),
    };
    let say = |line: &str| evaluations.say(device_id, id, line);
    let answer = Box::pin(match_service::match_job(context, ask, &say)).await?;
    if let Some(read) = &answer.read_cv {
        evaluations.set_read_cv(read.clone());
    }
    Ok(outcome(&answer))
}

fn outcome(answer: &MatchAnswer) -> Outcome {
    let result = &answer.result;
    let said = if answer.matched_before {
        "Matched before: this is the saved match.".to_string()
    } else if let Some(why) = &answer.save_error {
        format!("Matched, but it couldn't be saved: {why}")
    } else if result.saved_id.is_none() {
        "Not saved: with no AI model set up on the computer, this is the free match, which isn't saved.".to_string()
    } else {
        "Saved to your matches.".to_string()
    };
    Outcome {
        match_id: result.saved_id,
        percent: result.score.percent,
        tag: result.score.tag.clone(),
        tag_name: result.score.tag_name.clone(),
        heading: result.heading.clone(),
        said,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_device_shortlist_is_the_apps() {
        assert_eq!(device_protocol::SHORTLIST, SHORTLIST);
    }
}
