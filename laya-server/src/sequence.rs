//! The model's input rows, built exactly as laya 0.4.0's `build_head` / `build_sequence` and
//! `collate_items` build them:
//!
//!   [CLS] <type> question: <instructions> [SEP] [MASK] opt0 [MASK] opt1 ... [SEP] <state> [SEP]
//!
//! One row per question, all sharing the once-tokenized state; the state is cut to the room the
//! head leaves inside max_len (its start is kept, or its end for a conversation turn list).

use std::fs;
use std::path::Path;

use serde_json::Value;
use tokenizers::Tokenizer;

use crate::question::Question;

/// Longest option text, in tokens after its [MASK] (laya's `truncation=True, max_length=48`).
const OPTION_TOKEN_CAP: usize = 48;

/// When fewer head tokens than this are left for the instructions, every option is cut evenly.
const MIN_INSTRUCTION_BUDGET: i64 = 16;

/// The fewest tokens an evenly cut option keeps, its [MASK] included.
const MIN_TOKENS_PER_CUT_OPTION: i64 = 4;

/// The fewest instruction tokens the head keeps, however long the options are.
const MIN_INSTRUCTION_TOKENS: i64 = 8;

/// The tokenizer plus the special tokens laya places by hand.
pub struct Tokens {
    tokenizer: Tokenizer,
    /// The mask token's text; laya replaces it with a space in every text it encodes.
    mask_text: String,
    cls: i64,
    sep: i64,
    mask: i64,
    pub pad: i64,
}

impl Tokens {
    /// Load `<dir>/tokenizer/tokenizer.json`, taking the special tokens' names from
    /// tokenizer_config.json beside it, as transformers' AutoTokenizer does.
    pub fn load(checkpoint_dir: &Path) -> Result<Self, String> {
        let dir = checkpoint_dir.join("tokenizer");
        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| format!("cannot load {}: {e}", dir.join("tokenizer.json").display()))?;
        let config_path = dir.join("tokenizer_config.json");
        let config: Value = fs::read_to_string(&config_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .ok_or_else(|| format!("cannot read {}", config_path.display()))?;
        let name = |key: &str| {
            config
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("{} has no {key}", config_path.display()))
        };
        let id = |text: &str| {
            tokenizer
                .token_to_id(text)
                .map(i64::from)
                .ok_or_else(|| format!("the tokenizer has no {text} token"))
        };
        let mask_text = name("mask_token")?;
        Ok(Self {
            cls: id(&name("cls_token")?)?,
            sep: id(&name("sep_token")?)?,
            mask: id(&mask_text)?,
            pad: id(&name("pad_token")?)?,
            mask_text,
            tokenizer,
        })
    }

    /// laya's `encode_text(tok, text.replace(mask, " "), add_special_tokens=False)`.
    fn encode(&self, text: &str) -> Result<Vec<i64>, String> {
        let text = text.replace(&self.mask_text, " ");
        let encoding = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| format!("tokenizer failed: {e}"))?;
        Ok(encoding.get_ids().iter().map(|&id| i64::from(id)).collect())
    }
}

/// What the state looked like before and after the clamp (laya's truncation stats).
#[derive(Debug, Clone, PartialEq)]
pub struct StateStats {
    pub state_tokens: usize,
    pub state_tokens_dropped: usize,
}

/// What the head budget did to the options (laya's `options` stats).
#[derive(Debug, Clone, PartialEq)]
pub struct OptionStats {
    pub options: usize,
    /// Options that still have a token span of their own after the cut.
    pub options_distinct: usize,
    /// The cut applied to every option, or None when there was none.
    pub tokens_per_option: Option<usize>,
}

/// One question's row before padding.
#[derive(Debug, Clone)]
pub struct Row {
    pub input_ids: Vec<i64>,
    /// Where each option's [MASK] sits; the head scores these positions.
    pub markers: Vec<i64>,
    pub qtype: i64,
    pub options: OptionStats,
    pub state: StateStats,
}

/// The state as text: a string as is, anything else as JSON (laya's `serialize_state`).
pub fn serialize_state(state: &Value) -> String {
    match state {
        Value::String(s) => s.clone(),
        other => crate::text::python_json(other),
    }
}

/// One row per question for this state, refusing a question whose options don't all fit.
pub fn build_rows(
    tokens: &Tokens,
    state: &Value,
    questions: &[Question],
    max_len: usize,
    head_max_len: usize,
) -> Result<Vec<Row>, String> {
    // A conversation (a list of turns) keeps its most recent end; anything else its start.
    let truncate_left = state.is_array();
    let state_ids = tokens.encode(&serialize_state(state))?;
    let mut rows = Vec::with_capacity(questions.len());
    for question in questions {
        let row = build_sequence(
            tokens,
            &state_ids,
            question,
            max_len,
            head_max_len,
            truncate_left,
        )?;
        if row.markers.len() != question.options.len() {
            return Err(format!(
                "question {}: only {} of its {} option markers fit in max_len={max_len} with head_max_len={head_max_len} \
                 spent on the question; lower head_max_len, raise max_len, or use fewer options",
                crate::text::repr_str(&question.id),
                row.markers.len(),
                question.options.len()
            ));
        }
        rows.push(row);
    }
    Ok(rows)
}

/// laya's `build_sequence`: the head, then as much of the state as fits, then [SEP].
fn build_sequence(
    tokens: &Tokens,
    state_ids: &[i64],
    question: &Question,
    max_len: usize,
    head_max_len: usize,
    truncate_left: bool,
) -> Result<Row, String> {
    let (mut ids, markers, options) = build_head(tokens, question, head_max_len)?;
    // One place is kept for the [SEP] that closes the state.
    let room = max_len.saturating_sub(ids.len() + 1);
    let kept = if truncate_left {
        &state_ids[state_ids.len().saturating_sub(room)..]
    } else {
        &state_ids[..room.min(state_ids.len())]
    };
    ids.extend_from_slice(kept);
    ids.push(tokens.sep);
    ids.truncate(max_len);
    let markers = markers
        .into_iter()
        .filter(|&m| m < max_len as i64)
        .collect();
    Ok(Row {
        input_ids: ids,
        markers,
        qtype: question.kind.index() as i64,
        options,
        state: StateStats {
            state_tokens: state_ids.len(),
            state_tokens_dropped: state_ids.len() - kept.len(),
        },
    })
}

/// laya's `build_head`: `[CLS] <type> question: <instructions> [SEP] [MASK] opt0 ... [SEP]`,
/// with the marker position of each option.
fn build_head(
    tokens: &Tokens,
    question: &Question,
    head_max_len: usize,
) -> Result<(Vec<i64>, Vec<i64>, OptionStats), String> {
    let mut instruction_ids = tokens.encode(&format!(
        "{} question: {}",
        question.kind.name(),
        question.instructions
    ))?;
    let mut option_ids = Vec::with_capacity(question.options.len());
    for option in &question.options {
        let mut ids = vec![tokens.mask];
        let text_ids = tokens.encode(&format!(" {option}"))?;
        ids.extend(text_ids.into_iter().take(OPTION_TOKEN_CAP));
        option_ids.push(ids);
    }

    let head_budget = head_max_len as i64;
    let spent = |opts: &Vec<Vec<i64>>| opts.iter().map(|o| o.len() as i64).sum::<i64>();
    let mut instruction_budget = head_budget - spent(&option_ids);
    let mut tokens_per_option = None;
    if instruction_budget < MIN_INSTRUCTION_BUDGET {
        // Too many or too long options: cut every option evenly.
        let per = MIN_TOKENS_PER_CUT_OPTION
            .max((head_budget - MIN_INSTRUCTION_BUDGET).div_euclid(option_ids.len().max(1) as i64));
        for ids in &mut option_ids {
            ids.truncate(per as usize);
        }
        tokens_per_option = Some(per as usize);
        instruction_budget = head_budget - spent(&option_ids);
    }
    instruction_ids.truncate(MIN_INSTRUCTION_TOKENS.max(instruction_budget) as usize);

    let mut ids = vec![tokens.cls];
    ids.extend(instruction_ids);
    ids.push(tokens.sep);
    let mut markers = Vec::with_capacity(option_ids.len());
    for option in &option_ids {
        markers.push(ids.len() as i64);
        ids.extend_from_slice(option);
    }
    ids.push(tokens.sep);

    let mut distinct = option_ids.clone();
    distinct.sort();
    distinct.dedup();
    let stats = OptionStats {
        options: option_ids.len(),
        options_distinct: distinct.len(),
        tokens_per_option,
    };
    Ok((ids, markers, stats))
}

/// The rows padded into the graph's inputs, as laya's `collate_items` pads them.
pub struct Batch {
    pub rows: usize,
    pub seq_len: usize,
    pub markers: usize,
    pub input_ids: Vec<i64>,
    pub attention_mask: Vec<i64>,
    pub marker_pos: Vec<i64>,
    pub marker_mask: Vec<bool>,
    pub qtype: Vec<i64>,
}

pub fn collate(rows: &[Row], pad: i64) -> Batch {
    let seq_len = rows.iter().map(|r| r.input_ids.len()).max().unwrap_or(0);
    let markers = rows.iter().map(|r| r.markers.len()).max().unwrap_or(0);
    let mut batch = Batch {
        rows: rows.len(),
        seq_len,
        markers,
        input_ids: vec![pad; rows.len() * seq_len],
        attention_mask: vec![0; rows.len() * seq_len],
        marker_pos: vec![0; rows.len() * markers],
        marker_mask: vec![false; rows.len() * markers],
        qtype: rows.iter().map(|r| r.qtype).collect(),
    };
    for (i, row) in rows.iter().enumerate() {
        let start = i * seq_len;
        batch.input_ids[start..start + row.input_ids.len()].copy_from_slice(&row.input_ids);
        batch.attention_mask[start..start + row.input_ids.len()].fill(1);
        let start = i * markers;
        batch.marker_pos[start..start + row.markers.len()].copy_from_slice(&row.markers);
        batch.marker_mask[start..start + row.markers.len()].fill(true);
    }
    batch
}
