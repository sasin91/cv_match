//! "Calls and notes" on a saved match's page: its recordings with their transcripts, "Add a
//! recording" (an audio file, transcribed by Whisper on this computer), Transcribe again for one
//! that failed, and Delete. All of it through recordings::recording_service.

use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{Column, button, column, row, text};
use iced::{Center, Element, Fill, Task};

use super::style;
use super::widgets::{self, CARD_PADDING, ROW_PADDING};
use super::{GAP, Runner};
use crate::dashboard::Health;
use crate::recordings::recording_row::RecordingRow;
use crate::recordings::recording_service::{self, AUDIO_EXTENSIONS};

/// The card's title.
pub const TITLE: &str = "Calls and notes";
/// The line under the title.
const INTRO: &str = "Recordings of calls and voice messages about this job, turned into text on this computer. Nothing is sent anywhere.";
/// What the card says while a recording is being added.
const WORKING: &str = "Transcribing… The first time, Whisper and its model (about 0.6 GB) are downloaded first. Follow it in the task manager.";
/// What the card says when there are no recordings yet.
const EMPTY: &str = "No recordings yet. Add a call recording from your phone, or a voice message.";

/// The recordings of the saved match open on its page.
#[derive(Debug, Default)]
pub struct Recordings {
    /// The saved match they are about.
    pub match_id: Option<i64>,
    /// None until read; Err says why they couldn't be.
    rows: Option<Result<Vec<RecordingRow>, String>>,
    /// Adding, transcribing or deleting one: the buttons wait.
    busy: bool,
    /// What the last step came to.
    said: Option<(Health, String)>,
}

#[derive(Debug, Clone)]
pub enum RecMsg {
    Loaded(i64, Result<Arc<Vec<RecordingRow>>, String>),
    Add,
    /// The file picked in the dialog; None when it was closed.
    Picked(Option<PathBuf>),
    Transcribe(i64),
    Delete(i64),
    /// What adding, transcribing or deleting came to.
    Done(i64, Result<&'static str, String>),
}

impl Recordings {
    /// Starts reading match_id's recordings, forgetting the last match's.
    pub fn open(&mut self, match_id: i64, runner: &Runner) -> Task<RecMsg> {
        *self = Recordings {
            match_id: Some(match_id),
            ..Recordings::default()
        };
        load(match_id, runner)
    }

    pub fn update(&mut self, msg: RecMsg, runner: &Runner) -> Task<RecMsg> {
        let Some(match_id) = self.match_id else {
            return Task::none();
        };
        match msg {
            RecMsg::Loaded(of, rows) => {
                if of == match_id {
                    self.rows = Some(rows.map(Arc::unwrap_or_clone));
                }
                Task::none()
            }
            RecMsg::Add => {
                self.busy = true;
                self.said = None;
                Task::perform(pick_audio(), RecMsg::Picked)
            }
            RecMsg::Picked(None) => {
                self.busy = false;
                Task::none()
            }
            RecMsg::Picked(Some(path)) => {
                self.said = Some((Health::Neutral, WORKING.to_string()));
                runner.run(
                    move |context| async move {
                        recording_service::add_file(&context, match_id, &path)
                            .await
                            .map(|_| "The recording was added and transcribed.")
                    },
                    move |outcome| RecMsg::Done(match_id, flatten(outcome)),
                )
            }
            RecMsg::Transcribe(id) => {
                self.busy = true;
                self.said = Some((Health::Neutral, WORKING.to_string()));
                runner.run(
                    move |context| async move {
                        recording_service::transcribe(&context, id)
                            .await
                            .map(|()| "The recording was transcribed.")
                    },
                    move |outcome| RecMsg::Done(match_id, flatten(outcome)),
                )
            }
            RecMsg::Delete(id) => {
                self.busy = true;
                self.said = None;
                runner.run(
                    move |context| async move {
                        recording_service::delete(&context, id)
                            .await
                            .map(|()| "The recording was deleted.")
                    },
                    move |outcome| RecMsg::Done(match_id, flatten(outcome)),
                )
            }
            RecMsg::Done(of, outcome) => {
                if of != match_id {
                    return Task::none();
                }
                self.busy = false;
                self.said = Some(match outcome {
                    Ok(said) => (Health::Ready, said.to_string()),
                    Err(why) => {
                        crate::app_log::error(&format!("Calls and notes: {why}"));
                        (Health::Attention, why)
                    }
                });
                // Read again either way: a recording whose transcription failed is still kept.
                load(match_id, runner)
            }
        }
    }
}

fn flatten(
    outcome: Result<Result<&'static str, crate::service_error::ServiceError>, String>,
) -> Result<&'static str, String> {
    outcome.and_then(|done| done.map_err(|e| e.message))
}

fn load(match_id: i64, runner: &Runner) -> Task<RecMsg> {
    runner.run(
        move |context| async move { recording_service::of_match(&context, match_id).await.map(Arc::new) },
        move |rows| RecMsg::Loaded(match_id, rows.and_then(|rows| rows.map_err(|e| e.message))),
    )
}

/// An audio file picked in the Open file dialog; None when it was closed.
async fn pick_audio() -> Option<PathBuf> {
    let file = rfd::AsyncFileDialog::new()
        .set_title("Choose a call recording or voice message")
        .add_filter("Sound", AUDIO_EXTENSIONS)
        .pick_file()
        .await?;
    Some(file.path().to_path_buf())
}

/// The card.
pub fn view(recordings: &Recordings) -> Element<'_, RecMsg> {
    let add = button(text("Add a recording").size(style::BODY_SIZE))
        .style(style::standard_button)
        .on_press_maybe((!recordings.busy).then_some(RecMsg::Add));
    let mut body = Column::new().spacing(GAP).push(
        row![widgets::note(INTRO), add]
            .spacing(GAP)
            .align_y(Center)
            .padding(CARD_PADDING),
    );
    if let Some((health, said)) = &recordings.said {
        body = body.push(column![widgets::message_bar(*health, said.as_str())].padding([0.0, CARD_PADDING]));
    }
    body = body.push(match &recordings.rows {
        None => column![widgets::note("Reading the recordings…")]
            .padding(CARD_PADDING)
            .into(),
        Some(Err(why)) => column![widgets::errors_line(why.as_str())].padding(CARD_PADDING).into(),
        Some(Ok(rows)) if rows.is_empty() => column![widgets::note(EMPTY)].padding(CARD_PADDING).into(),
        Some(Ok(rows)) => widgets::rows(rows.iter().map(|row| recording_view(row, recordings.busy))),
    });
    widgets::section(TITLE, body.into())
}

fn recording_view(recording: &RecordingRow, busy: bool) -> Element<'_, RecMsg> {
    let id = recording.id;
    let mut actions = row![].spacing(GAP);
    if !recording.transcribed {
        actions = actions.push(
            button(text("Transcribe again").size(style::CAPTION_SIZE))
                .style(style::standard_button)
                .on_press_maybe((!busy).then_some(RecMsg::Transcribe(id))),
        );
    }
    actions = actions.push(
        button(text("Delete").size(style::CAPTION_SIZE))
            .style(style::danger_button)
            .on_press_maybe((!busy).then_some(RecMsg::Delete(id))),
    );
    let transcript = text(recording.transcript.as_str()).size(style::BODY_SIZE);
    let transcript: Element<'_, RecMsg> = if recording.transcribed {
        transcript.into()
    } else {
        transcript.style(style::secondary).into()
    };
    column![
        row![
            text(recording.title.as_str())
                .font(style::semibold())
                .size(style::BODY_SIZE)
                .width(Fill),
            actions
        ]
        .spacing(GAP)
        .align_y(Center),
        transcript,
    ]
    .spacing(6)
    .padding(ROW_PADDING)
    .into()
}
