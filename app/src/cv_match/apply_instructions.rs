//! How a job post asks to be applied to, when it asks for more than a click, ported from
//! modules/cv_match/Apply_instructions.php.
//!
//! Some posts tell applicants to send an email (to an address, with a subject line, a keyword,
//! or answers to a few questions) or to write a given word in the application, partly to sort
//! out people who only press Easy Apply. read() finds those instructions in the raw post text,
//! Danish or English, by rule (no model call), so the page can show them prominently and the
//! application can follow them.

use std::sync::LazyLock;

use fancy_regex::Regex;
use serde::Serialize;

use crate::php;

/// What a post can ask for beyond sending the application, in plain words, in this order.
pub const ASKS: [(&str, &str); 5] = [
    ("subject", "a specific subject line"),
    ("keyword", "a specific word or phrase"),
    ("questions", "answers to questions"),
    ("no_easy_apply", "no one-click applications"),
    ("proof", "proof that the post was read"),
];

/// Most sentences quoted back, questions included.
const MAX_LINES: usize = 10;

/// Most list lines taken as the questions a sentence introduces.
const MAX_QUESTIONS: usize = 8;

/// The post's instructions: by_email (the post asks for the application by email), email (the
/// address, when the instruction names one), subject and keyword (the quoted subject line or
/// word to include), asks (from ASKS), lines (the post's own sentences that say so, to quote) and
/// trap (something is asked beyond a plain "send it to this address").
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApplyInstructions {
    pub by_email: bool,
    pub email: String,
    pub subject: String,
    pub keyword: String,
    pub asks: Vec<&'static str>,
    pub lines: Vec<String>,
    pub trap: bool,
}

fn pattern(source: &str) -> Regex {
    Regex::new(source).unwrap_or_else(|e| panic!("{source}: {e}"))
}

static LINE_BREAK: LazyLock<Regex> = LazyLock::new(|| pattern(r"\r\n|[\n\x0B\x0C\r\x{85}\x{2028}\x{2029}]"));
static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"[\p{L}0-9._%+-]+@[\p{L}0-9-]+(?:\.[\p{L}0-9-]+)*\.\p{L}{2,}"));
static MAIL_WORD: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i)\b(?:e-?mail\w*|mail\w*|mail os|send os en mail)\b"));
// Words that say a sentence is about applying, not only about mail.
static APPLY: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"(?i)(?:ansøg\w*|\bsøg\w*|application|apply|applying|applicants?|\bcv\b|resume|résumé|motivation\w*|cover letter)",
    )
});
// Mailing or writing something: "send", "skriv", "email us".
static SEND: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"(?i)\b(?:send\w*|skriv\w*|write|writing|email us|mail us|mail os|include|inkluder\w*|vedhæft\w*|attach\w*)\b",
    )
});
// "Questions? Write to ...": a contact line, not an application instruction.
static CONTACT: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"(?i)(?:spørgsmål til stillingen|har du (?:yderligere |flere |nogen )?spørgsmål|hvis du har (?:yderligere |flere )?spørgsmål|if you have (?:any )?(?:further |more )?questions|questions about (?:the|this)|any questions|for (?:more|further) information|vil du vide mere|vil du høre mere|hører du gerne mere|feel free to (?:contact|reach)|kontakt(?:e)? gerne)",
    )
});
// What says a contact sentence is about applying after all.
static APPLY_THERE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)(?:ansøg\w*|application|apply|\bcv\b)"));
static CUES: LazyLock<[(&str, Regex); 5]> = LazyLock::new(|| {
    [
        (
            "subject",
            pattern(r"(?i)(?:subject|\bemne(?:t|felt\w*|linje\w*)?\b|\bmærk(?:et|es|e)?\b|overskrift\w*)"),
        ),
        (
            "keyword",
            pattern(
                r"(?i)(?:keyword|code ?word|password|secret word|kodeord\w*|stikord\w*|nøgleord\w*|\bthe word\b|\bordet\b|\bthe phrase\b|\bsætningen\b)",
            ),
        ),
        (
            "questions",
            pattern(
                r"(?i)(?:\banswer\w*|besvar\w*|svar på|svar på følgende|following questions|questions below|these questions|the questions|følgende spørgsmål|spørgsmålene|nedenstående spørgsmål)",
            ),
        ),
        (
            "no_easy_apply",
            pattern(
                r"(?i)(?:easy apply|one-click|1-click|et klik|(?:will not|won't|wont|cannot|can't) be (?:considered|read|processed)|(?:not|never) (?:be )?considered|tages ikke i betragtning|behandles ikke|bliver ikke behandlet|læses ikke|kun ansøgninger|only applications|ignored|ignoreres|automatically (?:rejected|discarded)|frasorteres)",
            ),
        ),
        (
            "proof",
            pattern(
                r"(?i)(?:you(?:'ve| have)? (?:actually )?read|read (?:the|this) (?:whole |entire )?(?:post|ad|job)|læst (?:hele )?(?:opslaget|annoncen|stillingsopslaget)|start (?:your|the) (?:application|cover letter|email|e-mail|mail|message) with|begin (?:your|the) (?:application|cover letter|email|e-mail|mail|message)|top of your (?:application|email|cover letter)|øverst i (?:din |ansøgningen|mailen)|start (?:din|ansøgningen|mailen) med|begynd (?:din|ansøgningen|mailen) med)",
            ),
        ),
    ]
});
static QUOTED: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r#"["“”«»„]([^"“”«»„\n]{1,80})["“”«»]|(?<![\p{L}])['‘’]([^'‘’\n]{1,60})['‘’](?![\p{L}])"#)
});
// A line in a list: "- ...", "1. ...", "• ...", or a question.
static LIST_LINE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\s*(?:[-*•–·]|\d{1,2}[.)])\s+\S|\?\s*$"));
static ENDS_WITH_COLON: LazyLock<Regex> = LazyLock::new(|| pattern(r":\s*$"));
static SENTENCE_BREAK: LazyLock<Regex> = LazyLock::new(|| pattern(r#"(?<=[.!?])\s+(?=[\p{Lu}"“„«0-9])"#));

fn matches(re: &Regex, text: &str) -> bool {
    re.is_match(text).unwrap_or(false)
}

fn cue(name: &str) -> &'static Regex {
    &CUES.iter().find(|(n, _)| *n == name).expect("a cue of CUES").1
}

/// One sentence's instruction (by_email, asks), or None when it gives none.
struct Found {
    by_email: bool,
    asks: Vec<&'static str>,
}

/// The post's application instructions, or None when it gives none.
pub fn read(text: &str) -> Option<ApplyInstructions> {
    let lines = split(&LINE_BREAK, text);
    let mut picked: Vec<String> = Vec::new();
    let mut asks: Vec<&'static str> = Vec::new();
    let mut by_email = false;
    let mut after_instruction = false;
    for (index, line) in lines.iter().enumerate() {
        for sentence in sentences(line) {
            // "Send it to job@x.dk. Skriv 'Backend' i emnefeltet.": the second sentence goes on
            // with the first.
            let found = instruction(&sentence).or_else(|| if after_instruction { follow_up(&sentence) } else { None });
            after_instruction = found.is_some();
            let Some(found) = found else {
                continue;
            };
            by_email = by_email || found.by_email;
            asks.extend(found.asks.iter().copied());
            let introduces_list = found.asks.contains(&"questions") || matches(&ENDS_WITH_COLON, &sentence);
            picked.push(sentence);
            // "Answer these questions:" takes the list that follows.
            if introduces_list && questions_after(&lines, index, &mut picked) {
                asks.push("questions");
            }
        }
    }
    if picked.is_empty() {
        return None;
    }
    let mut unique: Vec<String> = Vec::new();
    for sentence in picked {
        if !unique.contains(&sentence) {
            unique.push(sentence);
        }
    }
    let picked = unique;
    let quoted = picked.join("\n");
    let asks: Vec<&'static str> = ASKS
        .iter()
        .map(|(ask, _)| *ask)
        .filter(|ask| asks.contains(ask))
        .collect();
    let email = EMAIL
        .find(&quoted)
        .ok()
        .flatten()
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    let subject = if asks.contains(&"subject") {
        quoted_after(&picked, &[cue("subject")])
    } else {
        String::new()
    };
    let keyword = if asks.contains(&"keyword") || asks.contains(&"proof") {
        quoted_after(&picked, &[cue("keyword"), cue("proof")])
    } else {
        String::new()
    };
    Some(ApplyInstructions {
        by_email,
        email,
        subject,
        keyword,
        trap: !asks.is_empty(),
        asks,
        lines: picked.into_iter().take(MAX_LINES).collect(),
    })
}

/// The instructions as a few lines for a prompt, empty when there are none.
pub fn for_prompt(found: Option<&ApplyInstructions>) -> String {
    let Some(found) = found else {
        return String::new();
    };
    let quote: Vec<String> = found.lines.iter().map(|line| format!("> {line}")).collect();
    let asks = if found.asks.is_empty() {
        "nothing beyond sending it".to_string()
    } else {
        found
            .asks
            .iter()
            .map(|ask| ask_words(ask))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "How the post asks to be applied to (it asks for: {asks}):\n{}",
        quote.join("\n")
    )
}

/// An ask in plain words ("a specific subject line").
pub fn ask_words(ask: &str) -> &'static str {
    ASKS.iter()
        .find(|(name, _)| *name == ask)
        .map(|(_, words)| *words)
        .unwrap_or("")
}

fn cues_in(sentence: &str) -> Vec<&'static str> {
    CUES.iter()
        .filter(|(_, re)| matches(re, sentence))
        .map(|(ask, _)| *ask)
        .collect()
}

fn instruction(sentence: &str) -> Option<Found> {
    let has_email = matches(&EMAIL, sentence);
    let mail = has_email || matches(&MAIL_WORD, sentence);
    let apply = matches(&APPLY, sentence);
    let mut asks = cues_in(sentence);
    // A subject line only counts with mail or an application named.
    if asks.contains(&"subject") && !mail && !apply {
        asks.retain(|ask| *ask != "subject");
    }
    // "Har du spørgsmål, så skriv til ..." is the contact person, unless the sentence also says
    // to apply there.
    if matches(&CONTACT, sentence) && !matches(&APPLY_THERE, sentence) {
        return None;
    }
    if mail && (apply || !asks.is_empty()) && (has_email || matches(&SEND, sentence) || !asks.is_empty()) {
        return Some(Found { by_email: true, asks });
    }
    // Without mail: "write BANANA at the top of your application", "applications through
    // LinkedIn won't be considered".
    asks.retain(|ask| *ask != "questions" && *ask != "subject");
    if apply && !asks.is_empty() {
        return Some(Found { by_email: false, asks });
    }
    None
}

/// What a sentence right after an instruction adds to it, or None.
fn follow_up(sentence: &str) -> Option<Found> {
    if matches(&CONTACT, sentence) {
        return None;
    }
    let asks = cues_in(sentence);
    (!asks.is_empty()).then_some(Found { by_email: false, asks })
}

/// A line split into sentences; email addresses and "e.g." stay whole.
fn sentences(line: &str) -> Vec<String> {
    let line = php::trim(line);
    if line.is_empty() {
        return Vec::new();
    }
    split(&SENTENCE_BREAK, line)
        .into_iter()
        .map(|part| php::trim(&part).to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// text split at every match of re, as preg_split does.
fn split(re: &Regex, text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0;
    for found in re.find_iter(text).flatten() {
        parts.push(text[start..found.start()].to_string());
        start = found.end();
    }
    parts.push(text[start..].to_string());
    parts
}

/// The list lines right after line index, added to picked: the questions an instruction
/// introduces. Whether it found any.
fn questions_after(lines: &[String], index: usize, picked: &mut Vec<String>) -> bool {
    let mut found = 0;
    for line in lines.iter().skip(index + 1) {
        if found >= MAX_QUESTIONS {
            break;
        }
        let line = php::trim(line);
        if line.is_empty() {
            if found > 0 {
                break;
            }
            continue;
        }
        if !matches(&LIST_LINE, line) {
            break;
        }
        picked.push(line.to_string());
        found += 1;
    }
    found > 0
}

/// The first quoted text in a sentence matching one of patterns.
fn quoted_after(sentences: &[String], patterns: &[&Regex]) -> String {
    for sentence in sentences {
        for pattern in patterns {
            let Some(cue) = pattern.find(sentence).ok().flatten() else {
                continue;
            };
            // The quote after the cue ("subject: 'X'"), else before it ("'X' in the subject").
            let quote = QUOTED
                .captures_from_pos(sentence, cue.start())
                .ok()
                .flatten()
                .or_else(|| QUOTED.captures(sentence).ok().flatten());
            if let Some(quote) = quote {
                let first = quote.get(1).map(|m| m.as_str()).unwrap_or("");
                let text = if first.is_empty() {
                    quote.get(2).map(|m| m.as_str()).unwrap_or("")
                } else {
                    first
                };
                return php::trim(text).to_string();
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_contact_line_is_not_an_instruction() {
        assert_eq!(
            read("Har du spørgsmål til stillingen, så kontakt Peter på peter@firma.dk eller 12345678."),
            None
        );
    }

    #[test]
    fn the_questions_an_instruction_lists_are_quoted_with_it() {
        let found = read(
            "Om jobbet\n\nSend en mail til job@firma.dk og besvar følgende spørgsmål:\n\n- Hvorfor os?\n- Hvad er dit yndlingsværktøj?\n\nVi holder samtaler løbende.",
        );
        assert_eq!(
            for_prompt(found.as_ref()),
            "How the post asks to be applied to (it asks for: answers to questions):\n> Send en mail til job@firma.dk og besvar følgende spørgsmål:\n> - Hvorfor os?\n> - Hvad er dit yndlingsværktøj?"
        );
    }
}
