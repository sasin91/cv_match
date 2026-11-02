//! The Setup screen: the setup wizard (setup) in the window. Where the data is, which model the
//! AI runs on, the profile read from the CV, and how often the job scan runs by itself, all
//! through setup_steps, the functions the web page uses. The window opens on it while the app
//! hasn't been set up (setup_steps::is_first_run).

use std::sync::Arc;

use iced::widget::{Column, button, checkbox, column, container, pick_list, row, text, text_editor};
use iced::{Center, Element, Fill, Task};

use super::matching::{Picked, pick_file};
use super::style;
use super::widgets::{self, CARD_PADDING};
use super::{GAP, Runner};
use crate::dashboard::Health;
use crate::scan_schedule::{DEFAULT_INTERVAL_HOURS, INTERVALS};
use crate::setup::setup_steps::{self, AiStep, CvStep, DatabaseStep, ScheduleStep};

/// The CV box's height.
const CV_BOX_HEIGHT: f32 = 220.0;
/// The interval list's width.
const PICK_WIDTH: f32 = 220.0;

/// The four steps as read.
pub struct Steps {
    database: DatabaseStep,
    ai: AiStep,
    cv: CvStep,
    schedule: ScheduleStep,
}

/// An interval the job scan can run at, for the list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    hours: u32,
    label: &'static str,
}

impl std::fmt::Display for Interval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

/// The intervals offered (scan_schedule::INTERVALS).
fn intervals() -> Vec<Interval> {
    INTERVALS
        .iter()
        .map(|&(hours, label)| Interval { hours, label })
        .collect()
}

/// The interval the list starts on: the task's, or the default.
fn interval_of(hours: Option<u32>) -> Option<Interval> {
    let hours = hours.unwrap_or(DEFAULT_INTERVAL_HOURS);
    intervals().into_iter().find(|interval| interval.hours == hours)
}

#[derive(Default)]
pub struct State {
    /// None until read; Err says why it couldn't be.
    steps: Option<Result<Arc<Steps>, String>>,
    cv_text: text_editor::Content,
    /// The file the CV was picked from, saved with the profile.
    cv_name: String,
    /// "Replace my profile" is ticked.
    replace: bool,
    /// Why the CV couldn't be read, under its box.
    cv_error: Option<String>,
    interval: Option<Interval>,
    /// Work is running (reading the CV, the scheduler), so the buttons wait.
    busy: bool,
    said: Option<(Health, String)>,
}

#[derive(Clone)]
pub enum Msg {
    Load,
    Loaded(Result<Arc<Steps>, String>),
    ChooseCv,
    CvPicked(Result<Picked, String>),
    CvEdit(text_editor::Action),
    Replace(bool),
    ReadCv,
    CvRead(Result<String, String>),
    Interval(Interval),
    Schedule,
    Unschedule,
    /// What the scheduler said: Ok what to say, or Err why not.
    Scheduled(Result<String, String>),
    /// For the shell: go to Language models.
    ShowLanguageModels,
    /// For the shell: go to Local model.
    ShowLocalModel,
    /// For the shell: go to the Match screen.
    Finish,
    /// What another screen's work came to, said here (a key saved during setup).
    Tell(String),
}

pub fn update(state: &mut State, msg: Msg, runner: &Runner) -> Task<Msg> {
    match msg {
        Msg::Load => {
            return runner.run(
                |context| async move {
                    let ai = setup_steps::ai(&context)
                        .await
                        .map_err(|e| format!("The database couldn't answer: {e}"))?;
                    let cv = setup_steps::cv(&context).await?;
                    Ok(Arc::new(Steps {
                        database: setup_steps::database(&context),
                        ai,
                        cv,
                        schedule: setup_steps::schedule(&context).await,
                    }))
                },
                |read| Msg::Loaded(read.and_then(|steps| steps)),
            );
        }
        Msg::Loaded(steps) => {
            if let Ok(steps) = &steps {
                state.interval = interval_of(steps.schedule.interval_hours);
                state.replace = false;
            }
            if let Err(why) = &steps {
                crate::log_line!("setup: the steps couldn't be read: {why}");
            }
            // A read or the scheduler may still be running: its own answer frees the buttons.
            state.steps = Some(steps);
        }
        Msg::ChooseCv => {
            state.busy = true;
            return runner.run(
                |context| async move { pick_file(context).await },
                |picked| Msg::CvPicked(picked.and_then(|picked| picked.map_err(|e| e.message))),
            );
        }
        Msg::CvPicked(picked) => {
            state.busy = false;
            match picked {
                Ok(None) => {}
                Ok(Some((name, text))) => {
                    state.cv_text = text_editor::Content::with_text(&text);
                    state.cv_name = name;
                    state.cv_error = None;
                }
                Err(why) => state.cv_error = Some(why),
            }
        }
        Msg::CvEdit(action) => state.cv_text.perform(action),
        Msg::Replace(replace) => state.replace = replace,
        Msg::ReadCv => {
            state.busy = true;
            state.cv_error = None;
            state.said = Some((Health::Neutral, "Your AI model is reading the CV…".to_string()));
            let (cv_text, cv_name, replace) = (state.cv_text.text(), state.cv_name.clone(), state.replace);
            return runner.run(
                move |context| async move { setup_steps::read_cv(&context, &cv_text, &cv_name, replace).await },
                |answer| Msg::CvRead(answer.and_then(|said| said.map_err(|e| e.message))),
            );
        }
        Msg::CvRead(Ok(said)) => {
            state.busy = false;
            state.cv_text = text_editor::Content::new();
            state.cv_name.clear();
            state.said = Some((Health::Ready, said));
            return Task::done(Msg::Load);
        }
        Msg::CvRead(Err(why)) => {
            state.busy = false;
            state.said = None;
            state.cv_error = Some(why);
        }
        Msg::Interval(interval) => state.interval = Some(interval),
        Msg::Schedule => {
            let Some(interval) = state.interval else {
                return Task::none();
            };
            state.busy = true;
            return runner.run(
                move |context| async move { setup_steps::schedule_scan(&context, interval.hours).await },
                |answer| Msg::Scheduled(answer.and_then(|said| said.map_err(|e| e.message))),
            );
        }
        Msg::Unschedule => {
            state.busy = true;
            return runner.run(
                |context| async move { setup_steps::unschedule_scan(&context).await.map(str::to_string) },
                |answer| Msg::Scheduled(answer.and_then(|said| said.map_err(|e| e.message))),
            );
        }
        Msg::Scheduled(said) => {
            state.busy = false;
            state.said = Some(match said {
                Ok(said) => (Health::Ready, said),
                Err(why) => (Health::Attention, why),
            });
            return Task::done(Msg::Load);
        }
        // The shell changes screens.
        Msg::Tell(said) => state.said = Some((Health::Ready, said)),
        Msg::ShowLanguageModels | Msg::ShowLocalModel | Msg::Finish => {}
    }
    Task::none()
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let title = widgets::page_title(
        "Set up JobFinder",
        widgets::note("Four steps, then the dashboard. Everything stays on this computer."),
    );
    let mut screen = Column::new().spacing(GAP * 1.5).push(title);
    if let Some((health, said)) = &state.said {
        screen = screen.push(widgets::message_bar(*health, said.as_str()));
    }
    let steps = match &state.steps {
        None => return screen.push(widgets::note("Reading the setup…")).into(),
        Some(Err(why)) => {
            return screen
                .push(widgets::message_bar(Health::Attention, why.as_str()))
                .into();
        }
        Some(Ok(steps)) => steps,
    };
    screen
        .push(step("1. Your data", true, database_step(&steps.database)))
        .push(step("2. An AI model", !steps.ai.saved.is_empty(), ai_step(&steps.ai)))
        .push(step("3. Your CV", steps.cv.filled.is_some(), cv_step(&steps.cv, state)))
        .push(step(
            "4. Checking for new jobs",
            steps.schedule.interval_hours.is_some(),
            schedule_step(&steps.schedule, state),
        ))
        .push(
            row![
                space_fill(),
                button(text("Go to the dashboard").size(style::BODY_SIZE))
                    .padding([8.0, 16.0])
                    .style(style::primary_button)
                    .on_press(Msg::Finish),
            ]
            .align_y(Center),
        )
        .into()
}

fn space_fill<'a>() -> Element<'a, Msg> {
    iced::widget::space().width(Fill).into()
}

/// A step's card: its title with a done mark, and its body.
fn step<'a>(title: &'a str, done: bool, body: Element<'a, Msg>) -> Element<'a, Msg> {
    let heading = row![
        style::mark(if done { Health::Ready } else { Health::Neutral }),
        widgets::section_title(title),
    ]
    .spacing(10)
    .align_y(Center);
    column![
        heading,
        widgets::card(container(body).padding(CARD_PADDING).width(Fill).into())
    ]
    .spacing(10)
    .into()
}

fn database_step(database: &DatabaseStep) -> Element<'_, Msg> {
    let kept = if database.made_now {
        "A new database was made for you at"
    } else {
        "Your data is kept in"
    };
    column![
        text(format!("{kept} {}.", database.path)).size(style::BODY_SIZE),
        widgets::caption(
            "Your matches, profile and settings are in this one file. Your API keys are in Windows Credential Manager (your keyring on Linux), not in it."
        ),
    ]
    .spacing(6)
    .into()
}

fn ai_step(ai: &AiStep) -> Element<'_, Msg> {
    if !ai.saved.is_empty() {
        let mut body = column![text("The AI runs on:").size(style::BODY_SIZE)].spacing(6);
        for saved in &ai.saved {
            body = body.push(text(format!("• {saved}")).size(style::BODY_SIZE));
        }
        return body
            .push(
                button(text("Change it in Language models").size(style::BODY_SIZE))
                    .style(style::standard_button)
                    .on_press(Msg::ShowLanguageModels),
            )
            .into();
    }
    let advice = &ai.advice;
    let mut body = column![widgets::note(
        "The AI reads your CV and the job posts, and writes applications and résumés."
    )]
    .spacing(8);
    match &advice.local {
        Some(local) => {
            body = body
                .push(text("Suggested: this app's own model").font(style::semibold()).size(style::BODY_SIZE))
                .push(widgets::caption(format!(
                    "It runs on this computer, so it needs no API key and costs nothing per use, and your CV and the job posts stay here. This computer ({}) runs {} ({}): it {}.",
                    advice.computer, local.label, local.size, local.fit
                )))
                .push(
                    row![
                        button(text("Set up this app's own model").size(style::BODY_SIZE))
                            .style(style::primary_button)
                            .on_press(Msg::ShowLocalModel),
                        button(text("Use an OpenAI or Claude API key instead").size(style::BODY_SIZE))
                            .style(style::standard_button)
                            .on_press(Msg::ShowLanguageModels),
                    ]
                    .spacing(8),
                );
        }
        None => {
            body = body
                .push(text("Suggested: an OpenAI API key").font(style::semibold()).size(style::BODY_SIZE))
                .push(widgets::caption(format!(
                    "An OpenAI key runs on {} by default; a Claude key works too. This computer has no room on a graphics card for a model good enough for the AI match.",
                    advice.openai_model
                )))
                .push(
                    button(text("Add an API key").size(style::BODY_SIZE))
                        .style(style::primary_button)
                        .on_press(Msg::ShowLanguageModels),
                );
        }
    }
    body.into()
}

fn cv_step<'a>(cv: &'a CvStep, state: &'a State) -> Element<'a, Msg> {
    let mut body = Column::new().spacing(8);
    if let Some(filled) = &cv.filled {
        body = body.push(
            text(format!(
                "Your profile: {}. The match page matches with it, and applications and résumés are written from it.",
                setup_steps::filled_line(filled)
            ))
            .size(style::BODY_SIZE),
        );
    }
    if !cv.can_read {
        if cv.filled.is_none() {
            body = body.push(widgets::note("Set up an AI model first: reading the CV runs on it."));
        }
        return body.into();
    }
    let waiting = |msg: Msg| (!state.busy).then_some(msg);
    let picked = if state.cv_name.is_empty() {
        "A PDF or text file, or paste it below.".to_string()
    } else {
        format!("Read from {}.", state.cv_name)
    };
    body = body
        .push(
            text(if cv.filled.is_some() {
                "Read another CV into your profile"
            } else {
                "Read your CV into your profile"
            })
            .font(style::semibold())
            .size(style::BODY_SIZE),
        )
        .push(
            row![
                button(text("Choose a file").size(style::BODY_SIZE))
                    .style(style::standard_button)
                    .on_press_maybe(waiting(Msg::ChooseCv)),
                widgets::caption(picked),
            ]
            .spacing(10)
            .align_y(Center),
        )
        .push(
            text_editor(&state.cv_text)
                .placeholder("Paste your CV here")
                .height(CV_BOX_HEIGHT)
                .size(style::BODY_SIZE)
                .style(style::text_area)
                .on_action(Msg::CvEdit),
        )
        .push(widgets::caption(
            "Read by your AI model, which takes up to a minute. Leave out your CPR number, health, union membership and photos.",
        ));
    if let Some(why) = &state.cv_error {
        body = body.push(widgets::errors_line(why.clone()));
    }
    if cv.filled.is_some() {
        body = body.push(
            checkbox(state.replace)
                .label("Replace my profile with what this CV says")
                .size(16)
                .text_size(style::BODY_SIZE)
                .style(style::check_box)
                .on_toggle(Msg::Replace),
        );
    }
    let can_read = !state.cv_text.text().trim().is_empty() && (cv.filled.is_none() || state.replace);
    body.push(
        button(text("Read the CV into my profile").size(style::BODY_SIZE))
            .style(style::primary_button)
            .on_press_maybe(waiting(Msg::ReadCv).filter(|_| can_read)),
    )
    .into()
}

fn schedule_step<'a>(schedule: &'a ScheduleStep, state: &'a State) -> Element<'a, Msg> {
    let waiting = |msg: Msg| (!state.busy).then_some(msg);
    let mut body = Column::new().spacing(8);
    body = body.push(match schedule.interval_hours {
        Some(hours) => text(format!(
            "The job scan runs by itself: {}, while you're logged in to this computer.",
            setup_steps::interval_label(hours).to_lowercase()
        ))
        .size(style::BODY_SIZE),
        None => text(
            "The system's scheduler (Task Scheduler on Windows, a systemd timer on Linux) can run the job scan by itself, so new posts are waiting when you open the app.",
        )
        .size(style::BODY_SIZE)
        .style(style::secondary),
    });
    if let Some(why) = &schedule.error {
        body = body.push(widgets::errors_line(format!("Task Scheduler couldn't be read: {why}")));
    }
    let scheduled = schedule.interval_hours.is_some();
    let mut buttons = row![
        pick_list(intervals(), state.interval, Msg::Interval)
            .width(PICK_WIDTH)
            .text_size(style::BODY_SIZE)
            .style(style::pick_field)
            .menu_style(style::pick_menu),
        button(text(if scheduled { "Change" } else { "Check for new jobs" }).size(style::BODY_SIZE))
            .style(if scheduled {
                style::standard_button
            } else {
                style::primary_button
            })
            .on_press_maybe(waiting(Msg::Schedule)),
    ]
    .spacing(8)
    .align_y(Center);
    if scheduled {
        buttons = buttons.push(
            button(text("Stop checking by itself").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press_maybe(waiting(Msg::Unschedule)),
        );
    }
    body = body.push(buttons);
    if !schedule.task_run.is_empty() {
        body = body.push(widgets::caption(format!("The task runs {}", schedule.task_run)));
    }
    body.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_starts_on_the_tasks_interval_or_the_default() {
        assert_eq!(interval_of(Some(8)).map(|interval| interval.hours), Some(8));
        assert_eq!(
            interval_of(None).map(|interval| interval.hours),
            Some(DEFAULT_INTERVAL_HOURS)
        );
        assert_eq!(interval_of(Some(3)), None);
    }
}
