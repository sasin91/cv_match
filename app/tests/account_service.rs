//! The API key page's, the llama.cpp wizard's and the setup wizard's work answers the same without
//! a web server, as a desktop window will call it: messages to show and view models back, and
//! errors as a kind and a message.

use std::sync::{Arc, LazyLock};

use jobfinder::account::account_model::UseChoices;
use jobfinder::account::account_service::{self, KeyForm, SaveKey};
use jobfinder::account::key_vault::KeyVault;
use jobfinder::account::llama_setup_service;
use jobfinder::context::Context;
use jobfinder::db;
use jobfinder::server;
use jobfinder::service_error::ErrorKind;
use jobfinder::setup::setup_steps;
use jobfinder::taxonomy::Taxonomy;

static TAXONOMY: LazyLock<Arc<Taxonomy>> = LazyLock::new(|| Arc::new(Taxonomy::load()));

/// A key that looks like OpenAI's.
const OPENAI_KEY: &str = "sk-proj-0123456789abcdef";

/// The work's shared state on a new, empty in-memory database: a first run.
async fn context() -> Context {
    let db = db::open_in_memory().await.unwrap();
    let user_id = server::local_user(&db).await.unwrap();
    Context::new(db, TAXONOMY.clone(), user_id, KeyVault::in_memory())
}

fn openai_form(api_key: &str) -> KeyForm {
    KeyForm {
        provider: " openai ".to_string(),
        model: String::new(),
        api_key: api_key.to_string(),
        base_url: String::new(),
    }
}

#[tokio::test]
async fn a_key_that_cant_be_saved_comes_back_with_why_and_what_was_typed() {
    let context = context().await;
    let saved = account_service::save_key(&context, &openai_form("sk-short"))
        .await
        .unwrap();
    let SaveKey::Invalid { form, errors } = saved else {
        panic!("saved a short key: {saved:?}");
    };
    assert_eq!(form.provider, "openai");
    assert_eq!(errors, ["The API key field must be at least 20 characters in length."]);
}

#[tokio::test]
async fn a_saved_key_is_listed_and_can_be_deleted() {
    let context = context().await;
    let saved = account_service::save_key(&context, &openai_form(OPENAI_KEY))
        .await
        .unwrap();
    assert_eq!(
        saved,
        SaveKey::Saved {
            message: "Your OpenAI key is saved, for every use.".to_string(),
            setup_unfinished: true,
        }
    );
    let view = account_service::account_view(&context, None).await.unwrap();
    assert_eq!(view.saved.len(), 1);
    assert_eq!(view.saved[0].key_hint, "cdef");
    assert!(view.setup_unfinished);
    assert!(!view.key_uses.is_empty());
    let deleted = account_service::delete_key(&context, "openai").await.unwrap();
    assert_eq!(deleted, "Your OpenAI key is deleted.");
    let view = account_service::account_view(&context, None).await.unwrap();
    assert!(view.saved.is_empty());
    assert!(view.key_uses.is_empty());
}

#[tokio::test]
async fn which_key_each_use_runs_on_is_saved() {
    let context = context().await;
    account_service::save_key(&context, &openai_form(OPENAI_KEY))
        .await
        .unwrap();
    let none = UseChoices {
        resume: None,
        application: None,
        job_evaluation: None,
    };
    let said = account_service::save_uses(&context, &none).await.unwrap();
    assert_eq!(said, "Saved which key each use runs on.");
}

#[tokio::test]
async fn a_check_without_a_key_says_to_save_one() {
    let context = context().await;
    let error = account_service::check_key(&context, "openai").await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::NeedKey);
    assert_eq!(error.message, "There's no key to check: save one first.");
}

#[tokio::test]
async fn the_llama_wizard_needs_its_folder_and_a_known_model() {
    let context = Arc::new(context().await);
    let unknown = llama_setup_service::download_model(&context, "poster.gguf").unwrap_err();
    assert_eq!(unknown.kind, ErrorKind::Invalid);
    assert_eq!(unknown.message, "poster.gguf isn't one of the models offered.");
    let no_folder = llama_setup_service::llama_setup_view(&context).await.err().unwrap();
    assert_eq!(no_folder.message, "The app was started without its llama folder.");
    assert!(
        llama_setup_service::use_model(&context, "Qwen3-8B-Q4_K_M.gguf")
            .await
            .is_err()
    );
    assert!(llama_setup_service::progress(&context).is_none());
}

#[tokio::test]
async fn reading_a_cv_needs_a_whole_cv() {
    let context = context().await;
    let error = setup_steps::read_cv(&context, "Too short.", "cv.pdf", false)
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Invalid);
    assert!(
        error.message.starts_with("The CV field must be at least"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn the_scan_is_scheduled_and_unscheduled() {
    let context = context().await;
    let said = setup_steps::schedule_scan(&context, 4).await.unwrap();
    assert_eq!(
        said,
        "The job scan runs every 4 hours while you're logged in to this computer."
    );
    assert_eq!(setup_steps::schedule(&context).await.interval_hours, Some(4));
    let odd = setup_steps::schedule_scan(&context, 5).await.unwrap_err();
    assert_eq!(odd.kind, ErrorKind::Failed);
    assert_eq!(
        odd.message,
        "The job scan couldn't be scheduled: 5 hours isn't one of the intervals offered."
    );
    assert_eq!(
        setup_steps::unschedule_scan(&context).await.unwrap(),
        "The job scan no longer runs by itself."
    );
    assert_eq!(setup_steps::schedule(&context).await.interval_hours, None);
}
