//! The exported decision graph on ONNX Runtime: one session run per request, one row per
//! question, with the five inputs and two outputs export.py gives it.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use ort::environment::Environment;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;

use crate::sequence::Batch;

/// The width of the act head's output: act, escalate.
const ACT_OUTPUTS: usize = 2;

/// ONNX Runtime allows one environment per process; every session is built in it.
static ENVIRONMENT: OnceLock<Environment> = OnceLock::new();

pub struct Model {
    // ort's run() takes the session mutably; requests take turns, as laya-serve's gate does.
    session: Mutex<Session>,
}

/// The graph's two outputs, row-major: `logits` [rows, markers] and `act_logits` [rows, 2].
pub struct Scores {
    pub markers: usize,
    pub logits: Vec<f32>,
    pub act_logits: Vec<f32>,
}

impl Scores {
    /// One row's option logits, cut to the options that row actually has.
    pub fn option_logits(&self, row: usize, options: usize) -> &[f32] {
        &self.logits[row * self.markers..row * self.markers + options]
    }

    pub fn act_logits(&self, row: usize) -> &[f32] {
        &self.act_logits[row * ACT_OUTPUTS..(row + 1) * ACT_OUTPUTS]
    }
}

impl Model {
    /// Open `laya.onnx` (its .data file beside it) with every graph optimization on, as
    /// laya's ONNXAgent does. `threads` caps ONNX Runtime's intra-op threads.
    pub fn load(path: &Path, threads: Option<usize>) -> Result<Self, String> {
        let environment = match ENVIRONMENT.get() {
            Some(environment) => environment,
            None => {
                let built = ort::init()
                    .with_name("laya-server")
                    .build()
                    .map_err(|e| e.to_string())?;
                ENVIRONMENT.get_or_init(|| built)
            }
        };
        let mut builder = Session::builder(environment)
            .and_then(|b| Ok(b.with_optimization_level(GraphOptimizationLevel::Level3)?))
            .map_err(|e| e.to_string())?;
        if let Some(n) = threads {
            builder = builder.with_intra_threads(n).map_err(|e| e.to_string())?;
        }
        let session = builder
            .commit_from_file(path)
            .map_err(|e| format!("cannot load {}: {e}", path.display()))?;
        Ok(Self {
            session: Mutex::new(session),
        })
    }

    pub fn run(&self, batch: &Batch) -> Result<Scores, String> {
        let tokens = [batch.rows as i64, batch.seq_len as i64];
        let markers = [batch.rows as i64, batch.markers as i64];
        let tensor_error = |e: ort::Error| e.to_string();
        let inputs = ort::inputs! {
            "input_ids" => Tensor::from_array((tokens, batch.input_ids.clone())).map_err(tensor_error)?,
            "attention_mask" => Tensor::from_array((tokens, batch.attention_mask.clone())).map_err(tensor_error)?,
            "marker_pos" => Tensor::from_array((markers, batch.marker_pos.clone())).map_err(tensor_error)?,
            "marker_mask" => Tensor::from_array((markers, batch.marker_mask.clone())).map_err(tensor_error)?,
            "qtype" => Tensor::from_array(([batch.rows as i64], batch.qtype.clone())).map_err(tensor_error)?,
        };
        let mut session = self
            .session
            .lock()
            .map_err(|_| "the model session is poisoned".to_string())?;
        let outputs = session
            .run(inputs)
            .map_err(|e| format!("inference failed: {e}"))?;
        let (_, logits) = outputs["logits"]
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        let (_, act_logits) = outputs["act_logits"]
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        Ok(Scores {
            markers: batch.markers,
            logits: logits.to_vec(),
            act_logits: act_logits.to_vec(),
        })
    }
}
