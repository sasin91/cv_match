//! One request end to end, as laya 0.4.0's `ONNXAgent.system_one`: check the questions,
//! build one row per question, run the graph once, decode, and report token usage.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::checkpoint::Checkpoint;
use crate::decode;
use crate::model::{Model, Scores};
use crate::question::{Question, parse_questions};
use crate::sequence::{Row, Tokens, build_rows, collate};

/// The `model` field of every answer, as ONNXAgent reports it.
pub const MODEL_NAME: &str = "laya-rl-agent-onnx";

/// The decision model never generates text.
const OUTPUT_TOKENS: u64 = 0;

#[derive(Debug)]
pub enum PredictError {
    /// The request can't be answered as asked (422, the message names the question).
    Invalid(String),
    /// The model failed (500).
    Failed(String),
}

pub struct Laya {
    pub checkpoint: Checkpoint,
    pub tokens: Tokens,
    model: Option<Model>,
}

impl Laya {
    /// The checkpoint and tokenizer only, enough to build rows (used by the sequence tests).
    pub fn without_model(checkpoint_dir: &Path) -> Result<Self, String> {
        Ok(Self {
            checkpoint: Checkpoint::load(checkpoint_dir)?,
            tokens: Tokens::load(checkpoint_dir)?,
            model: None,
        })
    }

    pub fn load(
        checkpoint_dir: &Path,
        onnx: &Path,
        threads: Option<usize>,
    ) -> Result<Self, String> {
        let mut laya = Self::without_model(checkpoint_dir)?;
        laya.model = Some(Model::load(onnx, threads)?);
        Ok(laya)
    }

    /// The questions, checked, and their rows for this state.
    pub fn encode(
        &self,
        state: &Value,
        questions: &Map<String, Value>,
    ) -> Result<(Vec<Question>, Vec<Row>), String> {
        let questions = parse_questions(questions)?;
        let rows = build_rows(
            &self.tokens,
            state,
            &questions,
            self.checkpoint.max_len,
            self.checkpoint.head_max_len,
        )?;
        Ok((questions, rows))
    }

    /// `{model, answers, usage}` for one state.
    pub fn predict(
        &self,
        state: &Value,
        questions: &Map<String, Value>,
    ) -> Result<Value, PredictError> {
        if questions.is_empty() {
            return Ok(json!({
                "model": MODEL_NAME,
                "answers": {},
                "usage": {"input_tokens": 0, "output_tokens": OUTPUT_TOKENS},
            }));
        }
        let (questions, rows) = self
            .encode(state, questions)
            .map_err(PredictError::Invalid)?;
        let scores = self.run(&rows).map_err(PredictError::Failed)?;

        let mut answers = Map::new();
        for (i, (question, row)) in questions.iter().zip(&rows).enumerate() {
            let logits = scores.option_logits(i, row.markers.len());
            answers.insert(
                question.id.clone(),
                decode::answer(&self.checkpoint, question, logits, scores.act_logits(i)),
            );
        }
        Ok(json!({ "model": MODEL_NAME, "answers": answers, "usage": usage(&questions, &rows) }))
    }

    /// The graph's raw scores for these rows, padded into one batch.
    pub fn run(&self, rows: &[Row]) -> Result<Scores, String> {
        let model = self.model.as_ref().ok_or("no ONNX model loaded")?;
        model.run(&collate(rows, self.tokens.pad))
    }
}

/// Token usage and what truncation did, as ONNXAgent reports it (#174 in laya).
fn usage(questions: &[Question], rows: &[Row]) -> Value {
    let input_tokens: usize = rows.iter().map(|r| r.input_ids.len()).sum();
    // Worst case: the questions share one state, not one head budget.
    let dropped = rows
        .iter()
        .map(|r| r.state.state_tokens_dropped)
        .max()
        .unwrap_or(0);
    let truncated: Vec<&str> = questions
        .iter()
        .zip(rows)
        .filter(|(_, r)| r.state.state_tokens_dropped > 0)
        .map(|(q, _)| q.id.as_str())
        .collect();
    let mut usage = json!({
        "input_tokens": input_tokens,
        "output_tokens": OUTPUT_TOKENS,
        "state_tokens": rows[0].state.state_tokens,
        "state_tokens_dropped": dropped,
        "truncated": dropped > 0,
        "truncated_questions": truncated,
    });
    // Only when a question lost options to the head budget, as laya does.
    let collapsed: Map<String, Value> = questions
        .iter()
        .zip(rows)
        .filter(|(_, r)| r.options.options_distinct < r.options.options)
        .map(|(q, r)| {
            let stats = json!({
                "total": r.options.options,
                "distinct": r.options.options_distinct,
                "tokens_per_option": r.options.tokens_per_option,
            });
            (q.id.clone(), stats)
        })
        .collect();
    if !collapsed.is_empty() {
        usage["options"] = Value::Object(collapsed);
    }
    usage
}
