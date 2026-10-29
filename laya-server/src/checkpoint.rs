//! The checkpoint's rl_agent_config.json: the token budgets and the fitted temperatures.
//!
//! Mirrors how laya 0.4.0's ONNXAgent reads it (onnx_agent.py `__init__`): budgets default to
//! 512/192 when absent, and every temperature is clamped into [0.5, 5] with a warning, because
//! the shipped `choice:11+` bucket (0.1006) would sharpen the logits ~10x.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

/// laya's default `max_len`: the longest sequence, in tokens, fed to the encoder.
const DEFAULT_MAX_LEN: usize = 512;

/// laya's default `head_max_len`: the token budget for instructions plus options.
const DEFAULT_HEAD_MAX_LEN: usize = 192;

/// A fitted temperature below this sharpens instead of softening (laya's TEMP_MIN).
pub const TEMPERATURE_MIN: f64 = 0.5;

/// The highest temperature laya applies (laya's TEMP_MAX).
pub const TEMPERATURE_MAX: f64 = 5.0;

/// What a missing or unreadable temperature falls back to: no scaling.
const NEUTRAL_TEMPERATURE: f64 = 1.0;

/// One temperature per question type: choice, score, noul.
pub const QUESTION_TYPE_COUNT: usize = 3;

/// The only option layout the exported graph takes (the parallel one needs extra inputs).
const SEQUENTIAL_LAYOUT: &str = "sequential";

#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub max_len: usize,
    pub head_max_len: usize,
    /// Per question type, indexed by `QuestionType::index()`.
    pub temperature: [f64; QUESTION_TYPE_COUNT],
    /// Per bucket such as "score:3-5" (see decode::temperature_bucket).
    pub temperature_by_options: HashMap<String, f64>,
}

impl Checkpoint {
    /// Read `<dir>/rl_agent_config.json`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("rl_agent_config.json");
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let cfg: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not JSON: {e}", path.display()))?;
        Self::from_json(&cfg)
    }

    pub fn from_json(cfg: &Value) -> Result<Self, String> {
        let layout = cfg
            .get("option_layout")
            .and_then(Value::as_str)
            .unwrap_or(SEQUENTIAL_LAYOUT);
        if layout != SEQUENTIAL_LAYOUT {
            return Err(format!(
                "option_layout={layout:?} is not supported by the ONNX runtime"
            ));
        }

        let raw_temperature = match cfg.get("temperature") {
            None => vec![Value::from(NEUTRAL_TEMPERATURE); QUESTION_TYPE_COUNT],
            Some(Value::Array(list)) if list.len() == QUESTION_TYPE_COUNT => list.clone(),
            Some(other) => {
                return Err(format!(
                    "temperature must be a list of {QUESTION_TYPE_COUNT} numbers, got {other}"
                ));
            }
        };
        let mut clamped = Vec::new();
        let mut temperature = [NEUTRAL_TEMPERATURE; QUESTION_TYPE_COUNT];
        for (i, raw) in raw_temperature.iter().enumerate() {
            temperature[i] = clamp_temperature(raw);
            note_clamp(
                &mut clamped,
                &format!("temperature[{i}]"),
                raw,
                temperature[i],
            );
        }

        let mut temperature_by_options = HashMap::new();
        if let Some(Value::Object(buckets)) = cfg.get("temperature_by_options") {
            for (bucket, raw) in buckets {
                let applied = clamp_temperature(raw);
                note_clamp(&mut clamped, bucket, raw, applied);
                temperature_by_options.insert(bucket.clone(), applied);
            }
        }
        if !clamped.is_empty() {
            eprintln!(
                "laya-server: this checkpoint ships temperatures outside [{TEMPERATURE_MIN}, {TEMPERATURE_MAX}] which would \
                 distort confidence; clamping {}. Treat confidence from the affected buckets as uncalibrated.",
                clamped.join(", ")
            );
        }

        Ok(Self {
            max_len: budget(cfg, "max_len", DEFAULT_MAX_LEN)?,
            head_max_len: budget(cfg, "head_max_len", DEFAULT_HEAD_MAX_LEN)?,
            temperature,
            temperature_by_options,
        })
    }
}

/// A usable temperature: confined to [0.5, 5], or 1.0 when it is not a number (laya's
/// `clamp_temperature`; JSON has no NaN or infinity, and a bool is not a number).
pub fn clamp_temperature(raw: &Value) -> f64 {
    match raw.as_f64() {
        Some(t) if t.is_finite() => t.clamp(TEMPERATURE_MIN, TEMPERATURE_MAX),
        _ => NEUTRAL_TEMPERATURE,
    }
}

/// Remember an entry the clamp changed, for the one warning at load.
fn note_clamp(clamped: &mut Vec<String>, name: &str, raw: &Value, applied: f64) {
    if raw.as_f64() != Some(applied) {
        clamped.push(format!("{name}={raw} -> {applied}"));
    }
}

/// A token budget from the config, or laya's default when it is absent.
fn budget(cfg: &Value, key: &str, default: usize) -> Result<usize, String> {
    match cfg.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .map(|n| n as usize)
            .ok_or_else(|| format!("{key} must be a positive integer, got {v}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clamps_out_of_range_temperatures() {
        assert_eq!(
            clamp_temperature(&json!(0.10058280825614929)),
            TEMPERATURE_MIN
        );
        assert_eq!(clamp_temperature(&json!(9)), TEMPERATURE_MAX);
        assert_eq!(clamp_temperature(&json!(1.25)), 1.25);
    }

    #[test]
    fn non_numbers_fall_back_to_no_scaling() {
        assert_eq!(clamp_temperature(&json!(true)), NEUTRAL_TEMPERATURE);
        assert_eq!(clamp_temperature(&json!("2")), NEUTRAL_TEMPERATURE);
        assert_eq!(clamp_temperature(&Value::Null), NEUTRAL_TEMPERATURE);
    }

    #[test]
    fn budgets_default_like_laya() {
        let cp = Checkpoint::from_json(&json!({})).unwrap();
        assert_eq!(
            (cp.max_len, cp.head_max_len),
            (DEFAULT_MAX_LEN, DEFAULT_HEAD_MAX_LEN)
        );
        assert_eq!(cp.temperature, [NEUTRAL_TEMPERATURE; QUESTION_TYPE_COUNT]);
    }

    #[test]
    fn reads_the_english_config() {
        let cp = Checkpoint::from_json(&json!({
            "max_len": 512, "head_max_len": 192,
            "temperature": [1.6, 1.25, 1.98],
            "temperature_by_options": {"choice:11+": 0.1006, "score:3-5": 1.25}
        }))
        .unwrap();
        assert_eq!(cp.temperature_by_options["choice:11+"], TEMPERATURE_MIN);
        assert_eq!(cp.temperature_by_options["score:3-5"], 1.25);
    }

    #[test]
    fn refuses_the_parallel_layout() {
        assert!(Checkpoint::from_json(&json!({"option_layout": "parallel"})).is_err());
    }
}
