//! The Rust port answers as the PHP app does: every case in tests/fixtures/parity.json was
//! written by tests/fixtures/generate.php from the PHP modules.

use std::sync::LazyLock;

use indexmap::IndexMap;
use recruiter::cv_match::cv_matcher::{self, CvMatcher, Verdicts};
use recruiter::cv_match::free_reader::{self, FreeReader, YearMonth};
use recruiter::cv_match::{apply_instructions, match_score};
use recruiter::taxonomy::{self, Taxonomy};
use serde::Serialize;
use serde_json::Value;

/// The fixed day the fixtures count "until now" to (generate.php's NOW).
const NOW: YearMonth = YearMonth { year: 2026, month: 10 };

/// How far two scores may drift apart: both sides compute in doubles, in the same order.
const TOLERANCE: f64 = 1e-12;

/// How many ranked results the fixtures recorded per phrase.
const RANK_LIMIT: usize = 3;

static FIXTURES: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(include_str!("fixtures/parity.json")).expect("parity.json is valid JSON"));
static TAXONOMY: LazyLock<Taxonomy> = LazyLock::new(Taxonomy::load);

fn cases(name: &str) -> &'static Vec<Value> {
    FIXTURES[name]
        .as_array()
        .unwrap_or_else(|| panic!("parity.json has no {name}"))
}

/// Equal as PHP's json_encode would print them: numbers by value, and an empty PHP array as
/// either [] or {}.
fn same(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => {
            (a.as_f64().unwrap_or(f64::NAN) - b.as_f64().unwrap_or(f64::NAN)).abs() <= TOLERANCE
        }
        (Value::Array(a), Value::Object(b)) | (Value::Object(b), Value::Array(a)) => a.is_empty() && b.is_empty(),
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y)),
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| same(v, w)))
        }
        _ => expected == actual,
    }
}

fn assert_same(what: &str, expected: &Value, actual: impl Serialize) {
    let actual = serde_json::to_value(actual).unwrap();
    assert!(
        same(expected, &actual),
        "{what}\n  PHP:  {}\n  Rust: {}",
        serde_json::to_string(expected).unwrap(),
        serde_json::to_string(&actual).unwrap()
    );
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

#[test]
fn words_and_content_words_split_as_php_does() {
    for case in cases("words") {
        let phrase = text(&case["text"]);
        assert_same(&format!("words({phrase:?})"), &case["words"], taxonomy::words(phrase));
        assert_same(
            &format!("content_words({phrase:?})"),
            &case["content"],
            TAXONOMY.content_words(phrase),
        );
    }
}

#[test]
fn edit_distance_counts_as_php_does() {
    for case in cases("distances") {
        let (a, b) = (text(&case["a"]), text(&case["b"]));
        let max = case["max"].as_u64().unwrap() as usize;
        assert_same(
            &format!("edit_distance({a:?}, {b:?}, {max})"),
            &case["distance"],
            taxonomy::edit_distance(a, b, max),
        );
    }
}

#[test]
fn the_taxonomy_ranks_every_phrase_as_php_does() {
    for case in cases("rank") {
        let phrase = text(&case["phrase"]);
        let kind = case["kind"].as_str();
        let results: Vec<Value> = TAXONOMY
            .rank(phrase, kind, RANK_LIMIT)
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.term.id,
                    "score": r.score,
                    "lexical": r.lexical,
                    "matched": r.matched,
                    "method": TAXONOMY.match_method(r, phrase),
                })
            })
            .collect();
        assert_same(&format!("rank({phrase:?}, {kind:?})"), &case["results"], results);
    }
}

#[test]
fn years_are_read_as_php_reads_them() {
    for case in cases("years_asked") {
        let line = text(&case["line"]);
        assert_same(
            &format!("years_asked({line:?})"),
            &case["years"],
            free_reader::years_asked(line),
        );
    }
    let cvs: IndexMap<&str, &str> = cases("readings")
        .iter()
        .filter(|r| r["kind"] == "cv")
        .map(|r| (text(&r["name"]), text(&r["text"])))
        .collect();
    for case in cases("years_worked") {
        let name = text(&case["name"]);
        let input = case["text"].as_str().or_else(|| cvs.get(name).copied()).unwrap();
        assert_same(
            &format!("years_worked({name:?})"),
            &case["years"],
            free_reader::years_worked(input, NOW),
        );
    }
}

#[test]
fn the_free_reader_reads_posts_and_cvs_as_php_does() {
    for case in cases("readings") {
        let name = text(&case["name"]);
        let input = text(&case["text"]);
        let mut reader = FreeReader::new(&TAXONOMY, NOW);
        let answer = if case["kind"] == "job" {
            serde_json::to_value(reader.job(input)).unwrap()
        } else {
            serde_json::to_value(reader.cv(input)).unwrap()
        };
        assert_same(&format!("{name}: answer"), &case["answer"], &answer);
        assert_same(
            &format!("{name}: known"),
            &case["known"],
            reader.known.keys().collect::<Vec<_>>(),
        );
    }
}

#[test]
fn every_post_and_cv_match_as_php_matches_them() {
    let posts: IndexMap<&str, &str> = cases("readings")
        .iter()
        .filter(|r| r["kind"] == "job")
        .map(|r| (text(&r["name"]), text(&r["text"])))
        .collect();
    let cvs: IndexMap<&str, &str> = cases("readings")
        .iter()
        .filter(|r| r["kind"] == "cv")
        .map(|r| (text(&r["name"]), text(&r["text"])))
        .collect();
    for case in cases("matches") {
        let what = format!("{} x {}", text(&case["post"]), text(&case["cv"]));
        let mut reader = FreeReader::new(&TAXONOMY, NOW);
        let job = reader.job(posts[text(&case["post"])]);
        let profile = reader.cv(cvs[text(&case["cv"])]);
        assert_same(&format!("{what}: job"), &case["job"], &job);
        assert_same(&format!("{what}: profile"), &case["profile"], &profile);

        let matcher = CvMatcher::new(&TAXONOMY, profile, reader.known.clone());
        let groups = cv_matcher::criteria(&job);
        assert_same(&format!("{what}: groups"), &case["groups"], &groups);
        let mut verdicts = Verdicts::new();
        for unit in cv_matcher::units(&groups) {
            if let Some(verdict) = matcher.decide(unit) {
                verdicts.insert(unit.id.clone(), verdict);
            }
        }
        assert_same(&format!("{what}: verdicts"), &case["verdicts"], &verdicts);
        let combined = cv_matcher::combine(&groups, &verdicts);
        assert_same(&format!("{what}: combined"), &case["combined"], &combined);
        assert_same(
            &format!("{what}: score"),
            &case["score"],
            cv_matcher::score(&groups, &combined),
        );
        assert_same(
            &format!("{what}: laya_summary"),
            &case["laya_summary"],
            cv_matcher::laya_summary(&job, &groups, &combined),
        );
        assert_same(
            &format!("{what}: soft_skills"),
            &case["soft_skills"],
            cv_matcher::soft_skills(&job),
        );
    }
}

#[test]
fn scores_print_as_php_prints_them() {
    for case in cases("scores") {
        let score = case["score"].as_f64().unwrap();
        assert_same(
            &format!("percent({score})"),
            &case["percent"],
            match_score::percent(score),
        );
        assert_same(
            &format!("match_tag({score})"),
            &case["tag"],
            cv_matcher::match_tag(score),
        );
    }
}

#[test]
fn application_instructions_are_read_as_php_reads_them() {
    for case in cases("apply_instructions") {
        let text = text(&case["text"]);
        assert_same(
            &format!("apply_instructions::read({text:?})"),
            &case["found"],
            apply_instructions::read(text),
        );
    }
}
