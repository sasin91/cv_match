//! Parity with laya 0.4.0 on the fixtures export/fixtures.py recorded (12 synthetic Recruiter
//! summaries, including one long enough to be truncated).
//!
//! The rows need only the checkpoint (LAYA_CHECKPOINT); the answers also need the exported
//! graph (LAYA_ONNX). Without them each test says so and passes, which is what CI sees.

use std::env;
use std::fs;
use std::path::PathBuf;

use laya_server::agent::Laya;
use serde_json::Value;

/// How far a raw logit from ONNX Runtime here may be from the one the Python run recorded:
/// the same graph and inputs, so only float32 summation order (threads, kernels) moves it,
/// about 1e-5 through 28 layers. Relative as well, since the act head's logits are in the
/// thousands. The 4-decimal answers are compared exactly in the test above.
const RAW_LOGIT_TOLERANCE: f64 = 1e-4;
const RAW_LOGIT_RELATIVE_TOLERANCE: f64 = 1e-5;

fn fixtures() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures.json");
    serde_json::from_str(&fs::read_to_string(path).expect("tests/fixtures.json"))
        .expect("fixtures are JSON")
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn ids(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .expect("a list")
        .iter()
        .map(|v| v.as_i64().expect("an integer"))
        .collect()
}

#[test]
fn rows_match_laya_exactly() {
    let Some(checkpoint) = env_path("LAYA_CHECKPOINT") else {
        eprintln!(
            "skipped: set LAYA_CHECKPOINT to the English checkpoint folder to compare the rows"
        );
        return;
    };
    let laya = Laya::without_model(&checkpoint).expect("checkpoint loads");
    let fixtures = fixtures();
    let questions = fixtures["questions"].as_object().unwrap();
    for state in fixtures["states"].as_array().unwrap() {
        let id = &state["id"];
        let (parsed, rows) = laya.encode(&state["state"], questions).expect("rows build");
        let padded_len = rows.iter().map(|r| r.input_ids.len()).max().unwrap();
        for (question, row) in parsed.iter().zip(&rows) {
            let want = &state["graph"][&question.id];
            assert_eq!(
                row.input_ids,
                ids(&want["input_ids"]),
                "state {id} {}: input_ids",
                question.id
            );
            assert_eq!(
                vec![1; row.input_ids.len()],
                ids(&want["attention_mask"]),
                "state {id} {}: attention_mask",
                question.id
            );
            assert_eq!(
                row.markers,
                ids(&want["marker_pos"]),
                "state {id} {}: marker_pos",
                question.id
            );
            assert_eq!(
                row.qtype,
                want["qtype"].as_i64().unwrap(),
                "state {id} {}: qtype",
                question.id
            );
            assert_eq!(
                padded_len as u64,
                want["padded_len"].as_u64().unwrap(),
                "state {id}: padded length"
            );
        }
    }
}

#[test]
fn answers_match_laya_to_four_decimals() {
    let (Some(checkpoint), Some(onnx)) = (env_path("LAYA_CHECKPOINT"), env_path("LAYA_ONNX"))
    else {
        eprintln!("skipped: set LAYA_CHECKPOINT and LAYA_ONNX to compare the answers");
        return;
    };
    let laya = Laya::load(&checkpoint, &onnx, None).expect("model loads");
    let fixtures = fixtures();
    let questions = fixtures["questions"].as_object().unwrap();
    for state in fixtures["states"].as_array().unwrap() {
        let id = &state["id"];
        let got = laya
            .predict(&state["state"], questions)
            .expect("prediction");
        let want = &state["onnx"];
        assert_eq!(got["model"], want["model"], "state {id}: model");
        assert_eq!(got["usage"], want["usage"], "state {id}: usage");
        assert_eq!(got["answers"], want["answers"], "state {id}: answers");
        // The PyTorch path laya-serve runs gave the same answers when the fixtures were made.
        assert_eq!(
            got["answers"], state["torch"]["answers"],
            "state {id}: answers vs PyTorch"
        );
    }
}

#[test]
fn raw_scores_match_the_recorded_graph_outputs() {
    let (Some(checkpoint), Some(onnx)) = (env_path("LAYA_CHECKPOINT"), env_path("LAYA_ONNX"))
    else {
        eprintln!("skipped: set LAYA_CHECKPOINT and LAYA_ONNX to compare the raw logits");
        return;
    };
    let laya = Laya::load(&checkpoint, &onnx, None).expect("model loads");
    let fixtures = fixtures();
    let questions = fixtures["questions"].as_object().unwrap();
    for state in fixtures["states"].as_array().unwrap() {
        let (parsed, rows) = laya.encode(&state["state"], questions).unwrap();
        let scores = laya.run(&rows).expect("graph runs");
        for (i, (question, row)) in parsed.iter().zip(&rows).enumerate() {
            let want = &state["graph"][&question.id];
            let pairs = [
                (scores.option_logits(i, row.markers.len()), &want["logits"]),
                (scores.act_logits(i), &want["act_logits"]),
            ];
            for (got, want) in pairs {
                for (g, w) in got.iter().zip(want.as_array().unwrap()) {
                    let w = w.as_f64().unwrap();
                    let diff = (f64::from(*g) - w).abs();
                    assert!(
                        diff <= RAW_LOGIT_TOLERANCE + RAW_LOGIT_RELATIVE_TOLERANCE * w.abs(),
                        "state {} {}: {g} vs {w}",
                        state["id"],
                        question.id
                    );
                }
            }
        }
    }
}
