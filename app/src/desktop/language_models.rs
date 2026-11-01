//! The Language models screen: the API key page (account) in the window. The saved keys with
//! Check and Delete, which key each use runs on, and the form that adds one, all through
//! account_service, the functions the web page uses.

use std::sync::Arc;

use iced::widget::{Column, button, column, container, pick_list, row, text, text_input};
use iced::{Center, Element, Fill, Task};

use super::style;
use super::widgets::{self, CARD_PADDING, ROW_PADDING};
use super::{GAP, Runner};
use crate::account::account_model::{PROVIDERS, USES, UseChoices};
use crate::account::account_service::{self, AccountView, KeyForm, SaveKey};
use crate::account::saved_key::{KeyUse, SavedKey};
use crate::dashboard::Health;
use crate::llm::{LLAMA_CPP, LOCAL};

/// A field's width in the key form.
const FIELD_WIDTH: f32 = 420.0;
/// The width of a use's key list.
const PICK_WIDTH: f32 = 280.0;

/// An entry in a list: what the form sends, and what the screen shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    value: String,
    label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The screen's state: the page's data as last read, and the forms as typed.
#[derive(Default)]
pub struct State {
    /// None until read; Err says why it couldn't be.
    view: Option<Result<Arc<AccountView>, String>>,
    provider: Option<Choice>,
    model: String,
    api_key: String,
    base_url: String,
    /// Why the key form couldn't be saved.
    errors: Vec<String>,
    /// Per use (account_model::USES, in order), the key picked for it.
    uses: Vec<Option<Choice>>,
    /// The provider whose Delete was pressed once, waiting for the second press.
    confirm_delete: Option<String>,
    /// Work is running (a save, a check), so the buttons wait.
    busy: bool,
    /// What the last click came to.
    said: Option<(Health, String)>,
}

#[derive(Clone)]
pub enum Msg {
    Load,
    Loaded(Result<Arc<AccountView>, String>),
    Provider(Choice),
    Model(String),
    ApiKey(String),
    BaseUrl(String),
    Save,
    Saved(Result<Arc<SaveKey>, String>),
    PickUse(usize, Choice),
    SaveUses,
    AskDelete(String),
    CancelDelete,
    Delete(String),
    Check(String),
    /// What a click came to: Ok said, or Err why not; a reload follows when reload.
    Said {
        said: Result<String, String>,
        reload: bool,
    },
    /// Goes to the Local model screen.
    ShowLocalModel,
}

pub fn update(state: &mut State, msg: Msg, runner: &Runner) -> Task<Msg> {
    match msg {
        Msg::Load => {
            return runner.run(
                |context| async move { account_service::account_view(&context, None).await.map(Arc::new) },
                |read| Msg::Loaded(read.and_then(|view| view.map_err(|e| e.message))),
            );
        }
        Msg::Loaded(view) => {
            if let Ok(view) = &view {
                fill_forms(state, view);
            }
            state.view = Some(view);
            state.busy = false;
        }
        Msg::Provider(choice) => state.provider = Some(choice),
        Msg::Model(model) => state.model = model,
        Msg::ApiKey(key) => state.api_key = key,
        Msg::BaseUrl(url) => state.base_url = url,
        Msg::Save => {
            state.busy = true;
            let form = KeyForm {
                provider: state.provider.as_ref().map(|c| c.value.clone()).unwrap_or_default(),
                model: state.model.clone(),
                api_key: state.api_key.clone(),
                base_url: state.base_url.clone(),
            };
            return runner.run(
                move |context| async move { account_service::save_key(&context, &form).await },
                |answer| Msg::Saved(answer.and_then(|saved| saved.map(Arc::new).map_err(|e| e.message))),
            );
        }
        Msg::Saved(Ok(saved)) => match &*saved {
            SaveKey::Invalid { errors, .. } => {
                state.errors = errors.clone();
                state.busy = false;
            }
            SaveKey::Saved { message, .. } => {
                state.errors.clear();
                state.api_key.clear();
                state.said = Some((Health::Ready, message.clone()));
                return Task::done(Msg::Load);
            }
        },
        Msg::Saved(Err(why))
        | Msg::Said {
            said: Err(why),
            reload: false,
        } => {
            state.said = Some((Health::Attention, why));
            state.busy = false;
        }
        Msg::PickUse(index, choice) => {
            if let Some(slot) = state.uses.get_mut(index) {
                *slot = Some(choice);
            }
        }
        Msg::SaveUses => {
            state.busy = true;
            let chosen = use_choices(&state.uses);
            return runner.run(
                move |context| async move { account_service::save_uses(&context, &chosen).await.map(str::to_string) },
                said(true),
            );
        }
        Msg::AskDelete(provider) => state.confirm_delete = Some(provider),
        Msg::CancelDelete => state.confirm_delete = None,
        Msg::Delete(provider) => {
            state.confirm_delete = None;
            state.busy = true;
            return runner.run(
                move |context| async move { account_service::delete_key(&context, &provider).await },
                said(true),
            );
        }
        Msg::Check(provider) => {
            state.busy = true;
            state.said = Some((Health::Neutral, "Asking the model for a tiny answer…".to_string()));
            return runner.run(
                move |context| async move { account_service::check_key(&context, &provider).await },
                said(false),
            );
        }
        Msg::Said { said, reload } => {
            state.said = Some(match said {
                Ok(said) => (Health::Ready, said),
                Err(why) => (Health::Attention, why),
            });
            state.busy = false;
            if reload {
                return Task::done(Msg::Load);
            }
        }
        // The shell changes screens.
        Msg::ShowLocalModel => {}
    }
    Task::none()
}

/// A service's answer as Msg::Said.
fn said(
    reload: bool,
) -> impl Fn(Result<Result<String, crate::service_error::ServiceError>, String>) -> Msg + Send + 'static {
    move |answer| Msg::Said {
        said: answer.and_then(|said| said.map_err(|e| e.message)),
        reload,
    }
}

/// The forms as the page fills them: the provider to add with its saved model and address, and
/// each use's picked key.
fn fill_forms(state: &mut State, view: &AccountView) {
    state.provider = view
        .providers
        .iter()
        .find(|option| option.selected)
        .map(|option| Choice {
            value: option.value.to_string(),
            label: option.label.to_string(),
        });
    state.model = view.model.clone();
    state.base_url = view.base_url.clone();
    state.uses = view.key_uses.iter().map(picked).collect();
}

/// The key a use's list has picked.
fn picked(key_use: &KeyUse) -> Option<Choice> {
    key_use
        .choices
        .iter()
        .find(|choice| choice.selected)
        .map(|choice| Choice {
            value: choice.value.clone(),
            label: choice.label.clone(),
        })
}

/// The picks as account_service takes them; "" (no key) and no pick run a use on none.
fn use_choices(uses: &[Option<Choice>]) -> UseChoices {
    let pick = |name: &str| {
        let index = USES.iter().position(|(use_, _)| *use_ == name)?;
        let choice = uses.get(index)?.as_ref()?;
        Some(choice.value.clone()).filter(|value| !value.is_empty())
    };
    UseChoices {
        resume: pick(USES[0].0),
        application: pick(USES[1].0),
        job_evaluation: pick(USES[2].0),
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let title = widgets::page_title(
        "Language models",
        widgets::note("The AI runs on your own OpenAI or Anthropic key, or on a model on this computer."),
    );
    let mut screen = Column::new().spacing(GAP * 1.5).push(title);
    if let Some((health, said)) = &state.said {
        screen = screen.push(widgets::message_bar(*health, said.as_str()));
    }
    let view = match &state.view {
        None => return screen.push(widgets::note("Reading your keys…")).into(),
        Some(Err(why)) => {
            return screen
                .push(widgets::message_bar(Health::Attention, why.as_str()))
                .into();
        }
        Some(Ok(view)) => view,
    };
    if let Some(why) = &view.vault_error {
        screen = screen.push(widgets::message_bar(
            Health::Attention,
            format!("Your saved key couldn't be read: {why}"),
        ));
    }
    if view.saved.is_empty() {
        screen = screen.push(advice(view));
    } else {
        screen = screen.push(widgets::section(
            "Your keys",
            widgets::rows(view.saved.iter().map(|key| saved_row(key, state))),
        ));
    }
    if !view.key_uses.is_empty() {
        screen = screen.push(widgets::section("Use my keys for", uses_card(view, state)));
    }
    screen.push(widgets::section("Add a key", key_form(view, state))).into()
}

/// Which model to run the AI on, while no key is saved (model_advice).
fn advice(view: &AccountView) -> Element<'_, Msg> {
    let advice = &view.advice;
    let body: Element<'_, Msg> = match &advice.local {
        Some(local) => column![
            text("Suggested: this app's own model").font(style::semibold()).size(style::BODY_SIZE),
            widgets::caption(format!(
                "It runs on this computer, so it needs no API key and costs nothing per use, and your CV and the job posts stay here. This computer ({}) runs {} ({}): it {}.",
                advice.computer, local.label, local.size, local.fit
            )),
            button(text("Set up this app's own model").size(style::BODY_SIZE))
                .style(style::primary_button)
                .on_press(Msg::ShowLocalModel),
        ]
        .spacing(8)
        .into(),
        None => column![
            text("Suggested: an OpenAI API key").font(style::semibold()).size(style::BODY_SIZE),
            widgets::caption(format!(
                "An OpenAI key runs on {} by default; a Claude key works too. This computer has no room on a graphics card for a model good enough for the AI match.",
                advice.openai_model
            )),
        ]
        .spacing(8)
        .into(),
    };
    widgets::card(container(body).padding(CARD_PADDING).into())
}

/// A saved key: its provider, last characters, model and address, and Check and Delete.
fn saved_row<'a>(key: &'a SavedKey, state: &'a State) -> Element<'a, Msg> {
    let mut about = Vec::new();
    if !key.key_hint.is_empty() {
        about.push(format!("key ending in …{}", key.key_hint));
    }
    if !key.model.is_empty() {
        about.push(format!("model {}", key.model));
    }
    if !key.base_url.is_empty() {
        about.push(format!("at {}", key.base_url));
    }
    about.push(format!("saved {}", key.saved_on));
    let mut lines = column![
        text(key.provider.as_str())
            .font(style::semibold())
            .size(style::BODY_SIZE),
        widgets::caption(about.join(" · ")),
    ]
    .spacing(2)
    .width(Fill);
    if key.unreadable {
        lines = lines.push(
            text("This key isn't in Windows Credential Manager anymore, so the AI can't use it. Save it again below.")
                .size(style::CAPTION_SIZE)
                .style(style::health_text(Health::Attention)),
        );
    }
    let waiting = |msg: Msg| (!state.busy).then_some(msg);
    let provider = key.provider_value.clone();
    let buttons = if state.confirm_delete.as_deref() == Some(key.provider_value.as_str()) {
        row![
            button(text("Cancel").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press(Msg::CancelDelete),
            button(text(format!("Delete the {} key", key.provider)).size(style::BODY_SIZE))
                .style(style::danger_button)
                .on_press_maybe(waiting(Msg::Delete(provider))),
        ]
    } else {
        let mut buttons = row![];
        if !key.unreadable {
            buttons = buttons.push(
                button(text("Check").size(style::BODY_SIZE))
                    .style(style::standard_button)
                    .on_press_maybe(waiting(Msg::Check(provider.clone()))),
            );
        }
        buttons.push(
            button(text("Delete").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press_maybe(waiting(Msg::AskDelete(provider))),
        )
    };
    let mark = if key.unreadable {
        Health::Attention
    } else {
        Health::Ready
    };
    row![style::mark(mark), lines, buttons.spacing(8)]
        .spacing(14)
        .align_y(Center)
        .padding(ROW_PADDING)
        .into()
}

/// Per use, the key it runs on.
fn uses_card<'a>(view: &'a AccountView, state: &'a State) -> Element<'a, Msg> {
    let mut list = Column::new().spacing(10);
    for (index, key_use) in view.key_uses.iter().enumerate() {
        let choices: Vec<Choice> = key_use
            .choices
            .iter()
            .map(|choice| Choice {
                value: choice.value.clone(),
                label: choice.label.clone(),
            })
            .collect();
        let selected = state.uses.get(index).cloned().flatten();
        list = list.push(
            row![
                text(key_use.label).size(style::BODY_SIZE).width(Fill),
                pick_list(choices, selected, move |choice| Msg::PickUse(index, choice))
                    .text_size(style::BODY_SIZE)
                    .padding(8)
                    .style(style::pick_field)
                    .menu_style(style::pick_menu)
                    .width(PICK_WIDTH),
            ]
            .align_y(Center),
        );
    }
    let body = column![
        list,
        widgets::caption(
            "With no key, the CV match reads job posts with the free reader instead of the AI, and résumés or applications aren't written."
        ),
        button(text("Save").size(style::BODY_SIZE))
            .style(style::primary_button)
            .on_press_maybe((!state.busy).then_some(Msg::SaveUses)),
    ]
    .spacing(12)
    .padding(CARD_PADDING);
    body.into()
}

/// The form that adds a key: the provider, then the fields it needs.
fn key_form<'a>(view: &'a AccountView, state: &'a State) -> Element<'a, Msg> {
    let providers: Vec<Choice> = PROVIDERS
        .iter()
        .map(|(value, label)| Choice {
            value: value.to_string(),
            label: label.to_string(),
        })
        .collect();
    let provider = state
        .provider
        .as_ref()
        .map(|choice| choice.value.as_str())
        .unwrap_or("");
    let mut form = Column::new().spacing(14).padding(CARD_PADDING);
    if !state.errors.is_empty() {
        form = form.push(widgets::errors(&state.errors));
    }
    form = form.push(widgets::field(
        "Provider",
        pick_list(providers, state.provider.clone(), Msg::Provider)
            .text_size(style::BODY_SIZE)
            .padding(8)
            .style(style::pick_field)
            .menu_style(style::pick_menu)
            .width(FIELD_WIDTH)
            .into(),
        None,
    ));
    if provider != LLAMA_CPP {
        form = form.push(widgets::field(
            "API key",
            text_input("sk-…", &state.api_key)
                .on_input(Msg::ApiKey)
                .secure(true)
                .size(style::BODY_SIZE)
                .padding(8)
                .style(style::text_field)
                .width(FIELD_WIDTH)
                .into(),
            Some("From platform.openai.com/api-keys or console.anthropic.com. It's kept in Windows Credential Manager and never shown again. A local model usually needs none."),
        ));
    }
    form = form.push(widgets::field(
        "Model (optional)",
        text_input(model_placeholder(view, provider), &state.model)
            .on_input(Msg::Model)
            .size(style::BODY_SIZE)
            .padding(8)
            .style(style::text_field)
            .width(FIELD_WIDTH)
            .into(),
        Some(if provider == LLAMA_CPP {
            "A file name from the models folder, or blank for the one that fits this computer best."
        } else {
            "A local model needs its name, like llama3.1:8b."
        }),
    ));
    if provider == LOCAL {
        form = form.push(widgets::field(
            "Address of the local model",
            text_input(view.base_url_placeholder, &state.base_url)
                .on_input(Msg::BaseUrl)
                .size(style::BODY_SIZE)
                .padding(8)
                .style(style::text_field)
                .width(FIELD_WIDTH)
                .into(),
            Some("Its OpenAI-compatible API: Ollama's is http://localhost:11434/v1, LM Studio's http://localhost:1234/v1."),
        ));
    }
    form.push(widgets::caption(
        "One key is kept per provider: saving a key for a provider that has one replaces only that one.",
    ))
    .push(
        button(text("Save key").size(style::BODY_SIZE))
            .padding([8.0, 16.0])
            .style(style::primary_button)
            .on_press_maybe((!state.busy).then_some(Msg::Save)),
    )
    .into()
}

/// The model field's grey hint: this app's own model takes a file name, the others a model name.
fn model_placeholder<'a>(view: &'a AccountView, provider: &str) -> &'a str {
    if provider == LLAMA_CPP {
        "Blank: the one that fits this computer best"
    } else {
        &view.model_placeholder
    }
}
