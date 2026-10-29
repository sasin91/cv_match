//! A request's questions: checked the way laya 0.4.0 checks them (`Agent._check_question`),
//! then turned into the option texts the model reads (`render_options`).
//!
//! Supported: noul, score and choice with string or structured instructions and criteria.
//! Refused with a message, where Python would accept them: `labels`, `option_order`, and
//! non-string choice labels (Python answers those under a different JSON key than the label).

use serde_json::{Map, Value};

use crate::text::{python_json, python_repr, render_criterion, repr_str};

/// The noul option texts when a question gives no `criteria` for that side.
const NOUL_FALSE_DEFAULT: &str = "no, the statement does not hold";
const NOUL_TRUE_DEFAULT: &str = "yes, the statement holds";

/// The labels in front of the noul option texts (laya's `_DEFAULT_NOUL_LABELS`).
const NOUL_FALSE_LABEL: &str = "false";
const NOUL_TRUE_LABEL: &str = "true";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionType {
    Choice,
    Score,
    Noul,
}

impl QuestionType {
    /// The type names in laya's sorted(QTYPES) order, for the "use one of" message.
    const NAMES_SORTED: [&'static str; 3] = ["choice", "noul", "score"];

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "choice" => Some(Self::Choice),
            "score" => Some(Self::Score),
            "noul" => Some(Self::Noul),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Choice => "choice",
            Self::Score => "score",
            Self::Noul => "noul",
        }
    }

    /// The graph's `qtype` input and the temperature index (laya's QTYPES).
    pub fn index(self) -> usize {
        match self {
            Self::Choice => 0,
            Self::Score => 1,
            Self::Noul => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Question {
    pub id: String,
    pub kind: QuestionType,
    /// The instructions as the model reads them: the string, or its JSON text.
    pub instructions: String,
    /// One rendered text per option, in label order (noul: false, then true).
    pub options: Vec<String>,
    /// Choice only: the labels, which are also the answer keys.
    pub choice_labels: Vec<String>,
    /// Score only: the level descriptions for the legend.
    pub levels: Vec<String>,
}

/// Check and render every question, in request order.
pub fn parse_questions(questions: &Map<String, Value>) -> Result<Vec<Question>, String> {
    questions
        .iter()
        .map(|(id, def)| parse_question(id, def))
        .collect()
}

fn parse_question(id: &str, def: &Value) -> Result<Question, String> {
    let qid = repr_str(id);
    if id.trim().is_empty() {
        return Err(format!("question id must be a non-empty string, got {qid}"));
    }
    let Value::Object(def) = def else {
        return Err(format!(
            "question {qid}: definition must be a dict, got {}",
            python_type(def)
        ));
    };
    let type_value = def.get("type").unwrap_or(&Value::Null);
    let Some(kind) = type_value.as_str().and_then(QuestionType::from_name) else {
        return Err(format!(
            "question {qid}: unknown type {}; use one of {:?}",
            python_repr(type_value),
            QuestionType::NAMES_SORTED
        )
        .replace('"', "'"));
    };
    let instructions = instructions_text(&qid, def.get("instructions"))?;
    for unsupported in ["labels", "option_order"] {
        if def.contains_key(unsupported) {
            return Err(format!(
                "question {qid}: '{unsupported}' is not supported by laya-server; drop it"
            ));
        }
    }

    let criteria = def.get("criteria").unwrap_or(&Value::Null);
    let mut question = Question {
        id: id.to_string(),
        kind,
        instructions,
        options: Vec::new(),
        choice_labels: Vec::new(),
        levels: Vec::new(),
    };
    match kind {
        QuestionType::Choice => render_choice(&qid, criteria, &mut question)?,
        QuestionType::Score => render_score(&qid, criteria, &mut question)?,
        QuestionType::Noul => question.options = render_noul(&qid, criteria)?,
    }
    Ok(question)
}

/// The instructions as text, refusing what laya refuses.
fn instructions_text(qid: &str, instructions: Option<&Value>) -> Result<String, String> {
    let what = "add the text the model should answer";
    match instructions {
        None => Err(format!("question {qid}: no 'instructions'; {what}")),
        Some(Value::Null) => Err(format!(
            "question {qid}: 'instructions' must not be None; {what}"
        )),
        Some(Value::String(s)) if s.trim().is_empty() => Err(format!(
            "question {qid}: 'instructions' must not be empty; {what}"
        )),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Array(a)) if a.is_empty() => Err(format!(
            "question {qid}: 'instructions' must not be empty; {what}"
        )),
        Some(Value::Object(o)) if o.is_empty() => Err(format!(
            "question {qid}: 'instructions' must not be empty; {what}"
        )),
        Some(other) => Ok(python_json(other)),
    }
}

/// Choice options: "label" or "label: description" (laya renders a null or "" description as none).
fn render_choice(qid: &str, criteria: &Value, question: &mut Question) -> Result<(), String> {
    let pairs: Vec<(String, Value)> = match criteria {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Value::Array(list) => {
            let mut labels = Vec::new();
            for (i, label) in list.iter().enumerate() {
                match label {
                    Value::String(s) => labels.push((s.clone(), Value::Null)),
                    Value::Null => {
                        return Err(format!(
                            "question {qid}: choice label {i} is null; a label is rendered as option text and used as \
                             the answer key, so it must be a string"
                        ));
                    }
                    other => {
                        return Err(format!(
                            "question {qid}: choice label {i} is {}; laya-server takes string labels only",
                            python_repr(other)
                        ));
                    }
                }
            }
            labels
        }
        _ => {
            return Err(format!(
                "question {qid}: a choice question takes 'criteria' as a dict of label -> description, or a list of labels"
            ));
        }
    };
    if pairs.is_empty() {
        return Err(format!(
            "question {qid}: a choice question needs at least one criterion"
        ));
    }
    for (i, (label, _)) in pairs.iter().enumerate() {
        if let Some(first) = pairs[..i].iter().position(|(l, _)| l == label) {
            return Err(format!(
                "question {qid}: choice label {i} ({}) repeats label {first}; the labels are the answer keys, so every \
                 option needs its own",
                repr_str(label)
            ));
        }
    }
    for (label, description) in pairs {
        let undescribed = description.is_null() || description.as_str() == Some("");
        question.options.push(if undescribed {
            label.clone()
        } else {
            format!("{label}: {}", render_criterion(&description))
        });
        question.choice_labels.push(label);
    }
    Ok(())
}

/// Score options: "level <i>: <description>", index 0 first.
fn render_score(qid: &str, criteria: &Value, question: &mut Question) -> Result<(), String> {
    let Value::Array(levels) = criteria else {
        return Err(format!(
            "question {qid}: a score question takes 'criteria' as a list of level descriptions, index 0 first"
        ));
    };
    if levels.is_empty() {
        return Err(format!(
            "question {qid}: a score question needs at least one level"
        ));
    }
    if let Some(i) = levels.iter().position(Value::is_null) {
        return Err(format!(
            "question {qid}: score level {i} is null; give every level a description, index 0 first"
        ));
    }
    for (i, level) in levels.iter().enumerate() {
        let text = render_criterion(level);
        question.options.push(format!("level {i}: {text}"));
        question.levels.push(text);
    }
    Ok(())
}

/// Noul options, always [false, true], each "label: description" with laya's defaults.
fn render_noul(qid: &str, criteria: &Value) -> Result<Vec<String>, String> {
    let mut descriptions = Map::new();
    match criteria {
        Value::Null => {}
        Value::Object(map) => {
            for (k, v) in map {
                descriptions.insert(k.to_lowercase(), v.clone());
            }
            let mut keys: Vec<&String> = descriptions.keys().collect();
            keys.sort();
            if keys
                .iter()
                .any(|k| *k != NOUL_FALSE_LABEL && *k != NOUL_TRUE_LABEL)
            {
                let shown: Vec<String> = keys.iter().map(|k| repr_str(k)).collect();
                return Err(format!(
                    "question {qid}: a noul question takes 'criteria' keyed only 'true'/'false' (either or both, and \
                     omitted is fine), got [{}]",
                    shown.join(", ")
                ));
            }
        }
        _ => {
            return Err(format!(
                "question {qid}: a noul question takes 'criteria' as a dict with optional 'true'/'false' descriptions, or omits it"
            ));
        }
    }
    let side = |key: &str, default: &str| match descriptions.get(key) {
        None | Some(Value::Null) => default.to_string(),
        Some(Value::String(s)) if s.is_empty() => default.to_string(),
        Some(v) => render_criterion(v),
    };
    Ok(vec![
        format!(
            "{NOUL_FALSE_LABEL}: {}",
            side(NOUL_FALSE_LABEL, NOUL_FALSE_DEFAULT)
        ),
        format!(
            "{NOUL_TRUE_LABEL}: {}",
            side(NOUL_TRUE_LABEL, NOUL_TRUE_DEFAULT)
        ),
    ])
}

/// Python's type name for a JSON value, as laya's messages print it.
fn python_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one(def: Value) -> Result<Question, String> {
        let Value::Object(map) = json!({ "q": def }) else {
            unreachable!()
        };
        parse_questions(&map).map(|mut qs| qs.remove(0))
    }

    #[test]
    fn renders_recruiters_questions() {
        let fit = one(json!({"type": "score", "instructions": "How strong?", "criteria": ["no fit", "good fit"]})).unwrap();
        assert_eq!(fit.options, ["level 0: no fit", "level 1: good fit"]);
        let meets = one(json!({"type": "noul", "instructions": "Does it?"})).unwrap();
        assert_eq!(
            meets.options,
            [
                "false: no, the statement does not hold",
                "true: yes, the statement holds"
            ]
        );
    }

    #[test]
    fn renders_choice_descriptions() {
        let q = one(json!({"type": "choice", "instructions": "Pick", "criteria": {"a": null, "b": "", "c": "third"}})).unwrap();
        assert_eq!(q.options, ["a", "b", "c: third"]);
        assert_eq!(q.choice_labels, ["a", "b", "c"]);
    }

    #[test]
    fn structured_instructions_read_as_json() {
        let q = one(json!({"type": "noul", "instructions": {"ask": "x"}})).unwrap();
        assert_eq!(q.instructions, r#"{"ask": "x"}"#);
    }

    #[test]
    fn errors_name_the_question() {
        let err = one(json!({"type": "bool", "instructions": "x"})).unwrap_err();
        assert_eq!(
            err,
            "question 'q': unknown type 'bool'; use one of ['choice', 'noul', 'score']"
        );
        assert!(
            one(json!({"type": "score", "instructions": "x"}))
                .unwrap_err()
                .starts_with("question 'q': a score")
        );
        assert!(
            one(json!({"type": "noul"}))
                .unwrap_err()
                .contains("no 'instructions'")
        );
        assert!(
            one(json!({"type": "noul", "instructions": "x", "criteria": {"maybe": "?"}})).is_err()
        );
        assert!(
            one(json!({"type": "choice", "instructions": "x", "criteria": ["a", "a"]}))
                .unwrap_err()
                .contains("repeats")
        );
    }
}
