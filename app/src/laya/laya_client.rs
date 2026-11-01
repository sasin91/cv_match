//! Laya, the decision model that refines a ranked match.
//! laya-server's model runs in this process, so the two questions go straight to `Laya::predict`.
//!
//! The state is matcher::laya_summary(): the code-computed verdicts in one English sentence pair,
//! which the English checkpoint separated with AUC 0.98 in the bake-off (eval/README.md). Only
//! the yes/no and 0-4 fit heads are asked; the interview/maybe/reject choice head was poor
//! everywhere.
//!
//! The model is the exported English checkpoint in one folder (`laya` next to the database
//! unless `--laya` names another): rl_agent_config.json, tokenizer/, and laya.onnx with its
//! laya.onnx.data. Without laya.onnx there, from_folder() gives None and the match goes on
//! without Laya.
//!
//! Laya_limit isn't ported: it kept one visitor from queueing everyone else behind the shared
//! Laya pod, and here the model answers only its own user.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use laya_server::agent::{Laya, PredictError};
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio::sync::OnceCell;

use crate::matches::match_score;

/// The exported graph in the model folder; its weights, laya.onnx.data, sit beside it.
pub const ONNX_FILE: &str = "laya.onnx";

/// The model folder's name next to the database.
pub const FOLDER: &str = "laya";

pub const FIT_LEVELS: [&str; 5] = ["no fit", "weak fit", "partial fit", "good fit", "excellent fit"];

/// The highest fit level, as a number: fit is read as an expected level from 0 to this.
const FIT_MAX: f64 = (FIT_LEVELS.len() - 1) as f64;

/// The decimals `meets` is reported to.
const MEETS_DECIMALS: i32 = 4;

/// The decimals `fit` is reported to.
const FIT_DECIMALS: i32 = 3;

/// Laya's reading of a match: `meets`, the probability of meeting every requirement, `fit`, the
/// expected level from 0 to 4, and `fit_label`, the nearest level's name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayaAnswer {
    pub meets: f64,
    pub fit: f64,
    pub fit_label: String,
}

/// What a match carries of Laya: its answer, or why there is none this time. Both are None
/// without a model.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LayaReading {
    pub laya: Option<LayaAnswer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub laya_error: Option<String>,
}

/// Where loading Laya's model stands.
#[derive(Debug, Clone, PartialEq)]
pub enum LayaState {
    /// Not loaded yet: loading in the background, or waiting for the first match.
    Loading,
    Loaded,
    /// It couldn't be loaded, and why.
    Failed(String),
}

/// The model in its folder, loaded once: in the background at start (load()), or by the first
/// match that needs it, which then waits for it.
pub struct LayaClient {
    folder: PathBuf,
    loaded: OnceCell<Result<Arc<Laya>, String>>,
}

impl LayaClient {
    /// The client for the model in folder, or None when there is no laya.onnx in it.
    pub fn from_folder(folder: &Path) -> Option<Self> {
        folder.join(ONNX_FILE).is_file().then(|| Self {
            folder: folder.to_path_buf(),
            loaded: OnceCell::new(),
        })
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The model, loading it the first time (a few seconds: its weights are 1.7 GB). A load
    /// that failed stays failed until the app starts again, and every match says why.
    pub async fn load(&self) -> Result<Arc<Laya>, String> {
        self.loaded
            .get_or_init(|| async {
                let folder = self.folder.clone();
                let loading = tokio::task::spawn_blocking(move || {
                    Laya::load(&folder, &folder.join(ONNX_FILE), None).map(Arc::new)
                });
                let loaded = loading.await.unwrap_or_else(|e| Err(e.to_string()));
                loaded.map_err(|e| format!("Laya couldn't load its model from {}: {e}", self.folder.display()))
            })
            .await
            .clone()
    }

    /// How far loading the model has come, for the window's status line.
    pub fn state(&self) -> LayaState {
        match self.loaded.get() {
            None => LayaState::Loading,
            Some(Ok(_)) => LayaState::Loaded,
            Some(Err(why)) => LayaState::Failed(why.clone()),
        }
    }

    /// Laya's answer for a state.
    pub async fn decide(&self, state: &str) -> Result<LayaAnswer, String> {
        let laya = self.load().await?;
        let state = Value::from(state);
        let answering = tokio::task::spawn_blocking(move || laya.predict(&state, &questions()));
        let response = match answering.await {
            Ok(Ok(response)) => response,
            Ok(Err(PredictError::Invalid(detail) | PredictError::Failed(detail))) => {
                return Err(format!("Laya failed: {detail}"));
            }
            Err(e) => return Err(format!("Laya failed: {e}")),
        };
        read_answer(&response)
    }
}

/// The questions, worded as in the bake-off (eval/laya/run_laya.py).
pub fn questions() -> Map<String, Value> {
    let meets =
        json!({"type": "noul", "instructions": "Does the candidate meet every required qualification for the job?"});
    let fit = json!({"type": "score", "instructions": "How strong a fit is this candidate for the job?", "criteria": FIT_LEVELS});
    Map::from_iter([("meets".to_string(), meets), ("fit".to_string(), fit)])
}

/// Laya's response as its meets, fit (clamped to the levels) and fit_label.
pub fn read_answer(response: &Value) -> Result<LayaAnswer, String> {
    let answers = &response["answers"];
    let (Some(meets), Some(fit)) = (answers["meets"]["noul"].as_f64(), answers["fit"]["score"].as_f64()) else {
        return Err("Laya answered without the meets and fit scores.".to_string());
    };
    let fit = fit.clamp(0.0, FIT_MAX);
    Ok(LayaAnswer {
        meets: match_score::round_to(meets, MEETS_DECIMALS),
        fit: match_score::round_to(fit, FIT_DECIMALS),
        fit_label: FIT_LEVELS[fit.round() as usize].to_string(),
    })
}

/// Laya's reading of a match's summary: nothing without a model, else its answer, or the error,
/// logged, when it failed (the score comes back without it).
pub async fn reading(laya: Option<&LayaClient>, summary: &str) -> LayaReading {
    let Some(laya) = laya else {
        return LayaReading::default();
    };
    match laya.decide(summary).await {
        Ok(answer) => LayaReading {
            laya: Some(answer),
            laya_error: None,
        },
        Err(e) => {
            crate::log_line!("match (Laya): {e}");
            LayaReading {
                laya: None,
                laya_error: Some(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // How answers are read and printed is checked in tests/parity.rs.

    #[test]
    fn no_client_without_the_graph() {
        assert!(LayaClient::from_folder(Path::new("no such folder")).is_none());
    }
}
