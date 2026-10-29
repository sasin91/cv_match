//! The graph's raw scores turned into laya 0.4.0's answers (onnx_agent.py `_decode_answers`).
//!
//! The arithmetic is float32 where numpy's is, so the 4-decimal answers come out the same:
//! the logits divided by the bucket's temperature, a softmax, then per type the probability of
//! "true", the expected level, or the most likely label.

use serde_json::{Map, Value, json};

use crate::checkpoint::Checkpoint;
use crate::question::{Question, QuestionType};

/// Decimal places laya rounds every reported number to.
const REPORTED_DECIMALS: usize = 4;

/// The floor laya clips probabilities to before taking their log (entropy confidence).
const LOG_FLOOR: f32 = 1e-12;

/// Option counts up to which a question falls in the "2", "3-5" and "6-10" buckets.
const TWO_OPTIONS: usize = 2;
const FEW_OPTIONS: usize = 5;
const SEVERAL_OPTIONS: usize = 10;

/// The noul option the answer reports the probability of ([false, true]).
const NOUL_TRUE_SLOT: usize = 1;

/// The act head's first output: the probability of acting on the answer.
const ACT_SLOT: usize = 0;

/// The temperature bucket for a question type and option count, e.g. "score:3-5".
pub fn temperature_bucket(kind: QuestionType, options: usize) -> String {
    let size = if options <= TWO_OPTIONS {
        "2"
    } else if options <= FEW_OPTIONS {
        "3-5"
    } else if options <= SEVERAL_OPTIONS {
        "6-10"
    } else {
        "11+"
    };
    format!("{}:{size}", kind.name())
}

/// The temperature applied: the bucket's if fitted, otherwise the question type's.
pub fn temperature(checkpoint: &Checkpoint, kind: QuestionType, options: usize) -> f64 {
    checkpoint
        .temperature_by_options
        .get(&temperature_bucket(kind, options))
        .copied()
        .unwrap_or(checkpoint.temperature[kind.index()])
}

/// numpy's `p = exp(z - max(z)); p / p.sum()` in float32.
pub fn softmax(z: &[f32]) -> Vec<f32> {
    let max = z.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exp: Vec<f32> = z.iter().map(|&v| (v - max).exp()).collect();
    let sum: f32 = exp.iter().sum();
    exp.iter().map(|&e| e / sum).collect()
}

/// laya's `confidence_from_probs`: 1 - H(p) / ln(k), the concentration of the distribution.
pub fn entropy_confidence(p: &[f32]) -> f64 {
    let k = p.len();
    if k < TWO_OPTIONS {
        return 1.0;
    }
    let entropy: f32 = -p
        .iter()
        .map(|&v| v * v.clamp(LOG_FLOOR, 1.0).ln())
        .sum::<f32>();
    f64::from((1.0 - entropy / (k as f64).ln() as f32).clamp(0.0, 1.0))
}

/// laya's `answer_confidence`: the probability on the reported answer, max(p).
pub fn answer_confidence(p: &[f32]) -> f64 {
    match p.iter().copied().reduce(f32::max) {
        Some(top) => f64::from(top.clamp(0.0, 1.0)),
        None => 1.0,
    }
}

/// Python's `round(x, 4)`: correctly rounded from the exact binary value, ties to even.
pub fn round4(x: f64) -> f64 {
    format!("{x:.REPORTED_DECIMALS$}").parse().unwrap_or(x)
}

/// One question's answer from its row of option logits and act logits.
pub fn answer(
    checkpoint: &Checkpoint,
    question: &Question,
    logits: &[f32],
    act_logits: &[f32],
) -> Value {
    let k = logits.len();
    let t = temperature(checkpoint, question.kind, k) as f32;
    let scaled: Vec<f32> = logits.iter().map(|&l| l / t).collect();
    let p = softmax(&scaled);
    let act = softmax(act_logits);

    let confidence = round4(entropy_confidence(&p));
    let answer_confidence = round4(answer_confidence(&p));
    let action = json!({ "act_probability": round4(f64::from(act[ACT_SLOT])) });

    match question.kind {
        QuestionType::Choice => {
            let best = argmax(&p);
            json!({
                "type": "choice",
                "choice": question.choice_labels[best],
                "probabilities": keyed(question.choice_labels.iter().cloned(), &p),
                "confidence": confidence,
                "answer_confidence": answer_confidence,
                "action": action,
            })
        }
        QuestionType::Score => {
            let expected: f64 = p
                .iter()
                .enumerate()
                .map(|(i, &v)| i as f64 * f64::from(v))
                .sum();
            let legend: Map<String, Value> = question
                .levels
                .iter()
                .enumerate()
                .map(|(i, l)| (i.to_string(), Value::from(l.clone())))
                .collect();
            json!({
                "type": "score",
                "score": round4(expected),
                "legend": legend,
                "probabilities": keyed((0..k).map(|i| i.to_string()), &p),
                "confidence": confidence,
                "answer_confidence": answer_confidence,
                "action": action,
            })
        }
        QuestionType::Noul => {
            let yes = f64::from(p[NOUL_TRUE_SLOT]);
            json!({
                "type": "noul",
                "noul": round4(yes),
                "confidence": round4(yes.max(1.0 - yes)),
                "answer_confidence": answer_confidence,
                "action": action,
            })
        }
    }
}

/// The first index of the largest value, as numpy's argmax.
fn argmax(p: &[f32]) -> usize {
    let mut best = 0;
    for (i, &v) in p.iter().enumerate() {
        if v > p[best] {
            best = i;
        }
    }
    best
}

fn keyed(keys: impl Iterator<Item = String>, p: &[f32]) -> Map<String, Value> {
    keys.zip(p)
        .map(|(k, &v)| (k, Value::from(round4(f64::from(v)))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::question::parse_questions;
    use std::collections::HashMap;

    fn checkpoint() -> Checkpoint {
        Checkpoint {
            max_len: 512,
            head_max_len: 192,
            temperature: [1.0, 2.0, 1.0],
            temperature_by_options: HashMap::from([("noul:2".to_string(), 2.0)]),
        }
    }

    fn question(def: Value) -> Question {
        let Value::Object(map) = json!({ "q": def }) else {
            unreachable!()
        };
        parse_questions(&map).unwrap().remove(0)
    }

    #[test]
    fn buckets_are_named_like_laya() {
        assert_eq!(temperature_bucket(QuestionType::Noul, 2), "noul:2");
        assert_eq!(temperature_bucket(QuestionType::Score, 5), "score:3-5");
        assert_eq!(temperature_bucket(QuestionType::Choice, 6), "choice:6-10");
        assert_eq!(temperature_bucket(QuestionType::Choice, 11), "choice:11+");
    }

    #[test]
    fn bucket_temperature_wins_over_the_type_default() {
        assert_eq!(temperature(&checkpoint(), QuestionType::Noul, 2), 2.0);
        assert_eq!(temperature(&checkpoint(), QuestionType::Score, 5), 2.0);
        assert_eq!(temperature(&checkpoint(), QuestionType::Choice, 3), 1.0);
    }

    #[test]
    fn uniform_probabilities_have_no_confidence() {
        let p = softmax(&[0.5, 0.5, 0.5, 0.5]);
        assert!((p[0] - 0.25).abs() < 1e-7);
        assert_eq!(round4(entropy_confidence(&p)), 0.0);
        assert_eq!(answer_confidence(&p), f64::from(p[0]));
    }

    #[test]
    fn rounds_like_python() {
        assert_eq!(round4(0.123449), 0.1234);
        assert_eq!(round4(0.12345), 0.1235); // 0.12345 is stored just above the tie
        assert_eq!(round4(2.0), 2.0);
    }

    #[test]
    fn noul_reports_the_true_side_after_temperature() {
        let q = question(json!({"type": "noul", "instructions": "x"}));
        // logits 0 and 2 at temperature 2: softmax(0, 1) -> true 0.7311
        let a = answer(&checkpoint(), &q, &[0.0, 2.0], &[10.0, -10.0]);
        assert_eq!(a["noul"], 0.7311);
        assert_eq!(a["confidence"], 0.7311);
        assert_eq!(a["answer_confidence"], 0.7311);
        assert_eq!(a["action"]["act_probability"], 1.0);
    }

    #[test]
    fn score_is_the_expected_level() {
        let q =
            question(json!({"type": "score", "instructions": "x", "criteria": ["a", "b", "c"]}));
        let a = answer(&checkpoint(), &q, &[0.0, 0.0, 0.0], &[0.0, 0.0]);
        assert_eq!(a["score"], 1.0);
        assert_eq!(a["legend"], json!({"0": "a", "1": "b", "2": "c"}));
        assert_eq!(a["probabilities"]["2"], 0.3333);
        assert_eq!(a["action"]["act_probability"], 0.5);
    }

    #[test]
    fn choice_picks_the_first_most_likely_label() {
        let q = question(
            json!({"type": "choice", "instructions": "x", "criteria": ["yes", "no", "maybe"]}),
        );
        let a = answer(&checkpoint(), &q, &[1.0, 3.0, 3.0], &[0.0, 0.0]);
        assert_eq!(a["choice"], "no");
        let keys: Vec<&String> = a["probabilities"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["yes", "no", "maybe"]);
    }
}
