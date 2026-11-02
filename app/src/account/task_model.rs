//! The model each task runs on (user_llm_use_models). A task runs on the key its use picks
//! (account_model::USES), on the model saved for the task, else on the task's default for this
//! app's own model when that file is in the models folder, else on the key's own model.
//!
//! Which use each task belongs to:
//! - read_job: reading a job post (match_service::extract_job), and the job scan's web search;
//!   job_evaluation.
//! - read_cv: reading a CV for the match (match_service::extract_cv); job_evaluation.
//! - judge: judging the requirements the taxonomy can't decide (ai_match); job_evaluation.
//! - application: writing an application (document_writer); application.
//! - resume: tailoring a résumé (document_writer), and reading a CV into the profile
//!   (profile_fill), which runs on the résumé key; resume.
//!
//! A saved model belongs to the key its use picked when it was saved: when a use moves to another
//! key, or its key is deleted, its tasks' models are forgotten, so a GGUF file name never goes to
//! OpenAI or a model name of one provider to another.

use sqlx::{SqliteConnection, SqlitePool};

use super::account_model::{APPLICATION, JOB_EVALUATION, RESUME};
use crate::llama::catalog::{GEMMA_4_E4B, QWEN3_4B};
use crate::llm::LLAMA_CPP;

/// What a task saves to run on its key's own model even when it has a default: not a model name
/// (names are letters, digits and . _ : / -, account_service), so never mistaken for one, and not
/// "" (nothing saved, which runs the default).
pub const KEYS_OWN_MODEL: &str = "=key";

/// Something the AI does, on its own model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Task {
    /// Its name in user_llm_use_models and in the forms (model_for_{name}).
    pub name: &'static str,
    /// The use (account_model::USES) whose key it runs on.
    pub use_: &'static str,
    /// What the pages call it.
    pub label: &'static str,
    /// The model file it runs on with this app's own model when the user saved none, when that
    /// file is in the models folder.
    pub llama_default: Option<&'static str>,
}

pub const READ_JOB: Task = Task {
    name: "read_job",
    use_: JOB_EVALUATION,
    label: "Reading job posts",
    llama_default: Some(GEMMA_4_E4B),
};

pub const READ_CV: Task = Task {
    name: "read_cv",
    use_: JOB_EVALUATION,
    label: "Reading CVs for the match",
    llama_default: Some(QWEN3_4B),
};

pub const JUDGE: Task = Task {
    name: "judge",
    use_: JOB_EVALUATION,
    label: "Judging the match",
    llama_default: Some(QWEN3_4B),
};

pub const APPLICATION_TASK: Task = Task {
    name: "application",
    use_: APPLICATION,
    label: "Writing job applications",
    llama_default: None,
};

pub const RESUME_TASK: Task = Task {
    name: "resume",
    use_: RESUME,
    label: "Tailoring résumés and reading your CV into the profile",
    llama_default: None,
};

/// Every task, in the order the pages list them.
pub const TASKS: [Task; 5] = [READ_JOB, READ_CV, JUDGE, APPLICATION_TASK, RESUME_TASK];

/// The task that writes kind (document_box::APPLICATION or RESUME).
pub fn writing(kind: &str) -> &'static Task {
    if kind == RESUME {
        &RESUME_TASK
    } else {
        &APPLICATION_TASK
    }
}

/// The model the task runs on, on a key of provider whose own model is key_model: saved (""
/// when none is; KEYS_OWN_MODEL is the key's model), else the task's default for this app's own
/// model when downloaded (the models folder's file names) has it, else the key's model.
pub fn model_for(task: &Task, provider: &str, key_model: &str, saved: &str, downloaded: &[String]) -> String {
    if saved == KEYS_OWN_MODEL {
        return key_model.to_string();
    }
    if !saved.is_empty() {
        return saved.to_string();
    }
    match task.llama_default {
        Some(default) if provider == LLAMA_CPP && is_downloaded(default, downloaded) => default.to_string(),
        _ => key_model.to_string(),
    }
}

/// Whether file is one of the models folder's files, as llama_server names them (case aside).
fn is_downloaded(file: &str, downloaded: &[String]) -> bool {
    downloaded.iter().any(|name| name.eq_ignore_ascii_case(file))
}

/// The model saved for each task, as (task, model); a task without one isn't listed.
pub async fn saved(db: &SqlitePool, user_id: i64) -> sqlx::Result<Vec<(String, String)>> {
    let rows = sqlx::query!(
        r#"SELECT use AS "task!", model FROM user_llm_use_models WHERE trongate_user_id = ?1 AND model != ''"#,
        user_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|row| (row.task, row.model)).collect())
}

/// The model saved for task, "" when none is.
pub async fn saved_for(db: &SqlitePool, user_id: i64, task: &Task) -> sqlx::Result<String> {
    let model = sqlx::query_scalar!(
        r#"SELECT model FROM user_llm_use_models WHERE trongate_user_id = ?1 AND use = ?2"#,
        user_id,
        task.name
    )
    .fetch_optional(db)
    .await?;
    Ok(model.unwrap_or_default())
}

/// Saves each (task, model); "" runs the task on its default again.
pub async fn save(db: &SqlitePool, user_id: i64, chosen: &[(&'static Task, String)]) -> sqlx::Result<()> {
    let mut tx = crate::db::write_transaction(db).await?;
    for (task, model) in chosen {
        sqlx::query!(
            r#"
            INSERT INTO user_llm_use_models (trongate_user_id, use, model) VALUES (?1, ?2, ?3)
            ON CONFLICT (trongate_user_id, use) DO UPDATE SET model = excluded.model
            "#,
            user_id,
            task.name,
            model
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Forgets the models saved for the tasks of each of uses, which now run on another key or on
/// none, so they run on their defaults. Runs on connection, so it can be in the caller's
/// transaction.
pub async fn forget_for_uses(connection: &mut SqliteConnection, user_id: i64, uses: &[&str]) -> sqlx::Result<()> {
    for task in TASKS.iter().filter(|task| uses.contains(&task.use_)) {
        sqlx::query!(
            r#"DELETE FROM user_llm_use_models WHERE trongate_user_id = ?1 AND use = ?2"#,
            user_id,
            task.name
        )
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::llm::{LOCAL, OPENAI};

    async fn with_user() -> SqlitePool {
        let db = db::open_in_memory().await.unwrap();
        sqlx::query("INSERT INTO trongate_users (id) VALUES (1)")
            .execute(&db)
            .await
            .unwrap();
        db
    }

    fn files(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn a_saved_model_runs_before_any_default() {
        let downloaded = files(&[GEMMA_4_E4B]);
        assert_eq!(
            model_for(&READ_JOB, LLAMA_CPP, "", "Qwen3-8B-Q4_K_M.gguf", &downloaded),
            "Qwen3-8B-Q4_K_M.gguf"
        );
        assert_eq!(model_for(&READ_JOB, OPENAI, "gpt-5.4-mini", "gpt-5.4", &[]), "gpt-5.4");
    }

    #[test]
    fn this_apps_own_model_reads_job_posts_on_gemma_and_judges_on_qwen3_4b_when_downloaded() {
        let downloaded = files(&["GEMMA-4-E4B-it-Q4_K_M.gguf", QWEN3_4B, "Qwen3-8B-Q4_K_M.gguf"]);
        let key_model = "Qwen3-8B-Q4_K_M.gguf";
        assert_eq!(model_for(&READ_JOB, LLAMA_CPP, key_model, "", &downloaded), GEMMA_4_E4B);
        assert_eq!(model_for(&READ_CV, LLAMA_CPP, key_model, "", &downloaded), QWEN3_4B);
        assert_eq!(model_for(&JUDGE, LLAMA_CPP, key_model, "", &downloaded), QWEN3_4B);
        assert_eq!(
            model_for(&APPLICATION_TASK, LLAMA_CPP, key_model, "", &downloaded),
            key_model
        );
        assert_eq!(model_for(&RESUME_TASK, LLAMA_CPP, "", "", &downloaded), "");
    }

    #[test]
    fn the_keys_own_model_turns_a_downloaded_default_off() {
        let downloaded = files(&[GEMMA_4_E4B, QWEN3_4B]);
        let key_model = "Qwen3-8B-Q4_K_M.gguf";
        assert_eq!(
            model_for(&READ_JOB, LLAMA_CPP, key_model, KEYS_OWN_MODEL, &downloaded),
            key_model
        );
        assert_eq!(model_for(&JUDGE, LLAMA_CPP, "", KEYS_OWN_MODEL, &downloaded), "");
        assert_eq!(model_for(&READ_JOB, LLAMA_CPP, key_model, "", &downloaded), GEMMA_4_E4B);
    }

    #[test]
    fn without_the_default_file_or_on_another_provider_the_keys_model_runs() {
        let downloaded = files(&["Qwen3-8B-Q4_K_M.gguf"]);
        assert_eq!(model_for(&READ_JOB, LLAMA_CPP, "", "", &downloaded), "");
        assert_eq!(
            model_for(&JUDGE, LOCAL, "llama3.1:8b", "", &files(&[QWEN3_4B])),
            "llama3.1:8b"
        );
    }

    #[test]
    fn writing_picks_its_kind_and_munin_is_no_tasks_default() {
        assert_eq!(writing(RESUME), &RESUME_TASK);
        assert_eq!(writing(APPLICATION), &APPLICATION_TASK);
        assert!(
            TASKS
                .iter()
                .all(|task| task.llama_default != Some("munin-qwen3.5-9B.Q4_K_M.gguf"))
        );
    }

    #[tokio::test]
    async fn the_table_takes_every_task() {
        let db = with_user().await;
        let every: Vec<(&'static Task, String)> = TASKS.iter().map(|task| (task, "m".to_string())).collect();
        save(&db, 1, &every).await.unwrap();
        assert_eq!(saved(&db, 1).await.unwrap().len(), TASKS.len());
    }

    #[tokio::test]
    async fn a_saved_model_is_kept_per_task_until_its_use_moves() {
        let db = with_user().await;
        save(
            &db,
            1,
            &[
                (&READ_JOB, GEMMA_4_E4B.to_string()),
                (&JUDGE, QWEN3_4B.to_string()),
                (&RESUME_TASK, String::new()),
            ],
        )
        .await
        .unwrap();
        assert_eq!(saved_for(&db, 1, &READ_JOB).await.unwrap(), GEMMA_4_E4B);
        assert_eq!(saved_for(&db, 1, &READ_CV).await.unwrap(), "");
        let mut listed = saved(&db, 1).await.unwrap();
        listed.sort();
        assert_eq!(
            listed,
            [
                ("judge".to_string(), QWEN3_4B.to_string()),
                ("read_job".to_string(), GEMMA_4_E4B.to_string())
            ],
            "a task saved with \"\" isn't listed"
        );

        save(&db, 1, &[(&READ_JOB, "Qwen3-8B-Q4_K_M.gguf".to_string())])
            .await
            .unwrap();
        assert_eq!(saved_for(&db, 1, &READ_JOB).await.unwrap(), "Qwen3-8B-Q4_K_M.gguf");

        save(&db, 1, &[(&APPLICATION_TASK, "gpt-5.4".to_string())])
            .await
            .unwrap();
        let mut connection = db.acquire().await.unwrap();
        forget_for_uses(&mut connection, 1, &[JOB_EVALUATION]).await.unwrap();
        drop(connection);
        assert_eq!(
            saved(&db, 1).await.unwrap(),
            [("application".to_string(), "gpt-5.4".to_string())]
        );
    }
}
