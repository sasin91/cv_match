//! Taxonomy lookup for the recruiter, ported from taxonomy/php/Taxonomy.php: turns a phrase
//! from a job post or a CV ("Erfaring med SAP", "projektleder", "Kørekort B") into ranked
//! skill, role and title terms.
//!
//! This is the lexical half of the PHP lookup, the half cv_match uses: Cv_matcher ranks with
//! no query vector, so the static embedding model (Static_model, index.q8.bin) is not needed.
//! Plain word matching against each term's labels and definition, so exact names like "SAP" or
//! "Kørekort B" are never outranked, and words also match with one or two typos
//! ("projektleeder"), which is what step 3 of the pitch calls typo tolerance.
//!
//! Danish specifics: æ, ø and å survive normalisation, text is NFC-normalised before word
//! matching (a decomposed "å" from a PDF matches), and Danish stopwords join the English ones.
//! "it" is not a stopword, because "IT" is a skill word in Danish ("IT-support", "IT-chef").

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// The shipped index (taxonomy/data/index.json, written by build-index.php).
const INDEX_JSON: &str = include_str!("../../../taxonomy/data/index.json");

/// How much each kind of evidence is worth. A whole label of the term appearing in the query is
/// the strongest signal there is; a word turning up in the definition is the weakest.
pub mod weights {
    /// The term's preferred label (da or en), as a phrase.
    pub const LABEL: f64 = 1.0;
    /// Another label of the term, as a phrase.
    pub const SYNONYM: f64 = 0.9;
    /// A word from any label.
    pub const KEYWORD: f64 = 0.25;
    /// A word from the definition.
    pub const TEXT: f64 = 0.1;
    /// Multiplier for a word matched with typos.
    pub const TYPO: f64 = 0.8;
}

/// Specificity of a phrase of several words: the base, plus this much per extra word
/// ("projektleder i byggeriet" says more than "projektleder").
const PHRASE_BASE: f64 = 1.0;
const PHRASE_PER_EXTRA_WORD: f64 = 0.5;
/// Specificity of a one-word phrase: the floor, plus this much times the word's rarity ("sap"
/// names one term, "ledelse" hundreds).
const WORD_FLOOR: f64 = 0.4;
const WORD_RARITY_SHARE: f64 = 0.6;
/// A label that is the whole query beats one merely inside it, by this factor.
const WHOLE_QUERY_FACTOR: f64 = 2.0;

/// Word lengths, in characters, from which a word may carry one or two typos.
const ONE_TYPO_FROM: usize = 5;
const TWO_TYPOS_FROM: usize = 9;

/// Words that say nothing about which term a phrase is about. Kept short on purpose: a word
/// missing from here only adds a little noise, while a skill word wrongly listed here could never
/// be matched. English "it" and Danish "it" are deliberately absent ("IT-support"), and so is "c"
/// ("C#"). English first, then Danish (the Snowball list, minus words that are also skill words).
const STOPWORDS: &str = "a about after all also an and any are as at be because been before but by can could \
    did do does doesn don each for from had has have how i if in into is isn its just \
    me more most my no not of on or other our out over s so some such t than that the their \
    them then there these they this those through to too under up us very was way we were \
    what when where which while who why will with would you your \
    ad af alle alt anden at blev blive bliver da de dem den denne der deres det dette dig din \
    disse dog du efter eller en end er et for fra ham han hans har havde have hende hendes her \
    hos hun hvad hvis hvor i ikke ind jeg jer jo kunne man mange med meget men mig min mine mit \
    mod ned noget nogle nu når og også om op os over på sig sin sine sit skal skulle som sådan \
    thi til ud under var vi vil ville vor være været";

/// Words job ads wrap around a skill ("erfaring med SAP", "strong knowledge of Python"). They are
/// dropped from the query only, never from the terms, so a term like "Erfaringsudveksling" is
/// unaffected.
const QUALIFIERS: &str = "erfaring erfaringer kendskab viden god gode godt stærk stærke solid solide indgående \
    grundlæggende evne evner evnen års år gerne flere \
    experience experienced knowledge strong good solid proven ability years year basic advanced";

/// Runs of letters and digits.
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}]+").unwrap());

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Label {
    pub text: String,
    pub lang: String,
    pub preferred: bool,
}

/// Which kind of label a phrase is: the preferred one or a synonym.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    Label,
    Synonym,
    Keyword,
    Text,
}

impl Field {
    fn weight(self) -> f64 {
        match self {
            Field::Label => weights::LABEL,
            Field::Synonym => weights::SYNONYM,
            Field::Keyword => weights::KEYWORD,
            Field::Text => weights::TEXT,
        }
    }
}

/// One label of a term, as words to match on.
#[derive(Debug, Clone)]
pub struct Phrase {
    pub text: String,
    pub lang: String,
    pub field: Field,
    pub words: Vec<String>,
}

/// A term of the index, prepared for matching.
#[derive(Debug, Clone)]
pub struct Term {
    pub id: i64,
    pub kind: String,
    pub parent_id: Option<i64>,
    pub labels: Vec<Label>,
    pub definition: Option<String>,
    pub phrases: Vec<Phrase>,
    /// Words of any label, in order of first appearance.
    pub keywords: IndexSet<String>,
    /// Words of the definition.
    pub text: IndexSet<String>,
}

impl Term {
    /// The preferred label in `lang`, else the first label (Free_reader's label()).
    pub fn label(&self, lang: &str) -> &str {
        self.labels
            .iter()
            .find(|l| l.lang == lang && l.preferred)
            .or(self.labels.first())
            .map_or("", |l| l.text.as_str())
    }

    /// The preferred Danish label, else the first one (Cv_matcher's preferred()).
    pub fn preferred_da(&self) -> &str {
        self.labels
            .iter()
            .find(|l| l.preferred && l.lang == "da")
            .or(self.labels.first())
            .map_or("", |l| l.text.as_str())
    }
}

#[derive(Deserialize)]
struct Index {
    terms: Vec<IndexTerm>,
}

#[derive(Deserialize)]
struct IndexTerm {
    id: i64,
    kind: String,
    parent_id: Option<i64>,
    labels: Vec<Label>,
    #[serde(default)]
    definition: Option<String>,
}

/// What lexical evidence matched: a phrase (label or synonym) or a single word.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Matched {
    pub text: String,
    pub field: Field,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    pub typos: usize,
}

/// One ranked term with the evidence behind it.
#[derive(Debug, Clone)]
pub struct Ranked<'a> {
    pub term: &'a Term,
    pub score: f64,
    pub lexical: f64,
    pub matched: Vec<Matched>,
}

/// An index word close to a query word, and how many edits away.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub word: String,
    pub distance: usize,
}

/// How a phrase mapped onto a term, in job_post_terms.match_method's codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Exact,
    Synonym,
    Fuzzy,
    Embedding,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Exact => "exact",
            Method::Synonym => "synonym",
            Method::Fuzzy => "fuzzy",
            Method::Embedding => "embedding",
        }
    }
}

pub struct Taxonomy {
    pub terms: Vec<Term>,
    /// Word => number of terms it appears in, in order of first appearance.
    frequency: IndexMap<String, usize>,
    /// Index words by length in characters, for typo lookups.
    by_length: HashMap<usize, Vec<String>>,
    stopwords: HashSet<&'static str>,
    qualifiers: HashSet<&'static str>,
}

impl Taxonomy {
    /// The shipped index, built into the program.
    pub fn load() -> Self {
        Self::from_json(INDEX_JSON).expect("the built-in taxonomy index is valid")
    }

    /// Prepares an index (index.json's shape) for ranking: phrase lists, word sets and
    /// document frequencies.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        let index: Index = serde_json::from_str(json)?;
        let stopwords: HashSet<&'static str> = STOPWORDS.split_whitespace().collect();
        let qualifiers: HashSet<&'static str> = QUALIFIERS.split_whitespace().collect();
        let mut terms = Vec::with_capacity(index.terms.len());
        let mut frequency: IndexMap<String, usize> = IndexMap::new();
        for raw in index.terms {
            let mut phrases = Vec::new();
            let mut keywords = IndexSet::new();
            for label in &raw.labels {
                let words = words(&label.text);
                if words.iter().all(|w| stopwords.contains(w.as_str())) {
                    continue;
                }
                keywords.extend(words.iter().cloned());
                phrases.push(Phrase {
                    text: label.text.clone(),
                    lang: label.lang.clone(),
                    field: if label.preferred { Field::Label } else { Field::Synonym },
                    words,
                });
            }
            let text: IndexSet<String> = words(raw.definition.as_deref().unwrap_or("")).into_iter().collect();
            for word in keywords.iter().chain(text.iter().filter(|w| !keywords.contains(*w))) {
                *frequency.entry(word.clone()).or_insert(0) += 1;
            }
            terms.push(Term {
                id: raw.id,
                kind: raw.kind,
                parent_id: raw.parent_id,
                labels: raw.labels,
                definition: raw.definition,
                phrases,
                keywords,
                text,
            });
        }
        let mut by_length: HashMap<usize, Vec<String>> = HashMap::new();
        for word in frequency.keys() {
            by_length.entry(word.chars().count()).or_default().push(word.clone());
        }
        Ok(Self {
            terms,
            frequency,
            by_length,
            stopwords,
            qualifiers,
        })
    }

    /// Ranks every term for `query`, optionally of one `kind` (skill, role, title), by lexical
    /// evidence alone (PHP's rank() with no query vector). Ties go to the lower term id.
    pub fn rank(&self, query: &str, kind: Option<&str>, limit: usize) -> Vec<Ranked<'_>> {
        let expanded = self.expand_query(query);
        let query_words = words(query);
        let content = self.content_words(query);
        let mut scored: Vec<Ranked<'_>> = self
            .terms
            .iter()
            .filter(|term| kind.is_none_or(|k| term.kind == k))
            .map(|term| {
                let (lexical, matched) = self.lexical_match(term, &expanded, &query_words, &content);
                Ranked {
                    term,
                    score: lexical,
                    lexical,
                    matched,
                }
            })
            .filter(|r| r.score > 0.0)
            .collect();
        scored.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.term.id.cmp(&b.term.id)));
        scored.truncate(limit);
        scored
    }

    /// How a query mapped onto a result: `exact` when the query (or its content words: "erfaring
    /// med SAP" is "sap") is the term's preferred label, `synonym` when it is another label,
    /// `fuzzy` when it is a label with typos, `embedding` otherwise.
    pub fn match_method(&self, result: &Ranked<'_>, query: &str) -> Method {
        let all = words(query);
        let content = self.content_words(query);
        for phrase in &result.term.phrases {
            if phrase.words == all || phrase.words == content {
                return if phrase.field == Field::Label {
                    Method::Exact
                } else {
                    Method::Synonym
                };
            }
        }
        for m in &result.matched {
            if m.typos > 0 && matches!(m.field, Field::Label | Field::Synonym) {
                return if words(&m.text).len() == content.len() {
                    Method::Fuzzy
                } else {
                    Method::Embedding
                };
            }
        }
        Method::Embedding
    }

    /// The words of a query worth matching on: no stopwords, no qualifiers, no repeats.
    pub fn content_words(&self, text: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for w in words(text) {
            if !self.stopwords.contains(w.as_str()) && !self.qualifiers.contains(w.as_str()) && !out.contains(&w) {
                out.push(w);
            }
        }
        out
    }

    /// Index words within the typo budget of `word`, for words the index does not contain as
    /// they are. Each comes with its distance.
    pub fn typo_neighbours(&self, word: &str) -> Vec<Candidate> {
        if self.frequency.contains_key(word) {
            return Vec::new();
        }
        let budget = typo_budget(word);
        if budget == 0 {
            return Vec::new();
        }
        let n = word.chars().count();
        let mut found = Vec::new();
        for len in n - budget..=n + budget {
            for candidate in self.by_length.get(&len).into_iter().flatten() {
                if typo_budget(candidate) == 0 {
                    continue;
                }
                let distance = edit_distance(word, candidate, budget);
                if distance <= budget {
                    found.push(Candidate {
                        word: candidate.clone(),
                        distance,
                    });
                }
            }
        }
        found
    }

    /// The query's words with typos corrected towards the index: each query word maps to itself
    /// and to any index word within its typo budget.
    pub fn expand_query(&self, query: &str) -> IndexMap<String, Vec<Candidate>> {
        let mut expanded = IndexMap::new();
        for word in words(query) {
            if !expanded.contains_key(&word) {
                let mut candidates = vec![Candidate {
                    word: word.clone(),
                    distance: 0,
                }];
                candidates.extend(self.typo_neighbours(&word));
                expanded.insert(word, candidates);
            }
        }
        expanded
    }

    /// Lexical evidence for one term: a score in [0, 1) and what matched.
    fn lexical_match(
        &self,
        term: &Term,
        expanded: &IndexMap<String, Vec<Candidate>>,
        query_words: &[String],
        content: &[String],
    ) -> (f64, Vec<Matched>) {
        let mut matched = Vec::new();
        let mut raw = 0.0;
        let mut covered_by_phrase: HashSet<&str> = HashSet::new();

        for phrase in &term.phrases {
            // A phrase matches exactly, or with each of its words replaced by a typo'd query word
            // ("projektleeder" for "projektleder").
            let mut typos = 0;
            let mut hit = contains_phrase(query_words, &phrase.words);
            if !hit {
                let corrected: Vec<&str> = query_words
                    .iter()
                    .map(|w| {
                        let near = expanded
                            .get(w)
                            .into_iter()
                            .flatten()
                            .find(|c| c.distance > 0 && phrase.words.contains(&c.word));
                        match near {
                            Some(c) => {
                                typos += 1;
                                c.word.as_str()
                            }
                            None => w.as_str(),
                        }
                    })
                    .collect();
                hit = typos > 0 && contains_phrase(&corrected, &phrase.words);
            }
            if !hit {
                continue;
            }
            let whole = phrase.words == query_words || phrase.words == content;
            let count = phrase.words.len();
            let specificity = if count > 1 {
                PHRASE_BASE + PHRASE_PER_EXTRA_WORD * (count - 1) as f64
            } else {
                WORD_FLOOR + WORD_RARITY_SHARE * self.rarity(&phrase.words[0])
            } * if whole { WHOLE_QUERY_FACTOR } else { 1.0 };
            raw += phrase.field.weight() * specificity * if typos > 0 { weights::TYPO } else { 1.0 };
            matched.push(Matched {
                text: phrase.text.clone(),
                field: phrase.field,
                lang: Some(phrase.lang.clone()),
                typos,
            });
            covered_by_phrase.extend(phrase.words.iter().map(String::as_str));
        }

        for query_word in content {
            for Candidate { word, distance } in expanded.get(query_word).into_iter().flatten() {
                if covered_by_phrase.contains(word.as_str()) {
                    break;
                }
                let r = self.rarity(word) * if *distance > 0 { weights::TYPO } else { 1.0 };
                let field = if term.keywords.contains(word) {
                    Field::Keyword
                } else if term.text.contains(word) {
                    Field::Text
                } else {
                    continue;
                };
                raw += field.weight() * r;
                matched.push(Matched {
                    text: word.clone(),
                    field,
                    lang: None,
                    typos: *distance,
                });
                break;
            }
        }

        (1.0 - (-raw).exp(), matched)
    }

    /// Rarity of a word across terms, from 0 (in every term) towards 1 (in one). A word every
    /// term shares cannot tell terms apart.
    fn rarity(&self, word: &str) -> f64 {
        let n = self.terms.len() as f64;
        let df = self.frequency.get(word).copied().unwrap_or(0);
        if df == 0 {
            return 0.0;
        }
        if self.terms.len() < 2 {
            1.0
        } else {
            (n / df as f64).ln() / n.ln()
        }
    }
}

/// Lowercase runs of letters and digits, æøå kept, NFC first.
pub fn words(text: &str) -> Vec<String> {
    let lower = text.nfc().collect::<String>().to_lowercase();
    WORD.find_iter(&lower).map(|m| m.as_str().to_string()).collect()
}

/// Optimal string alignment distance (Levenshtein plus adjacent swaps), giving up once it is
/// certain to exceed `max`.
pub fn edit_distance(a: &str, b: &str, max: usize) -> usize {
    let s: Vec<char> = a.chars().collect();
    let t: Vec<char> = b.chars().collect();
    let (m, n) = (s.len(), t.len());
    if m.abs_diff(n) > max {
        return max + 1;
    }
    let mut prev2: Vec<usize> = Vec::new();
    let mut prev: Vec<usize> = (0..=n).collect();
    for i in 1..=m {
        let mut row = vec![i];
        let mut row_min = i;
        for j in 1..=n {
            let substitution = prev[j - 1] + usize::from(s[i - 1] != t[j - 1]);
            let mut d = (prev[j] + 1).min(row[j - 1] + 1).min(substitution);
            if i > 1 && j > 1 && s[i - 1] == t[j - 2] && s[i - 2] == t[j - 1] {
                d = d.min(prev2[j - 2] + 1);
            }
            row.push(d);
            row_min = row_min.min(d);
        }
        if row_min > max {
            return max + 1;
        }
        prev2 = prev;
        prev = row;
    }
    prev[n]
}

/// How many typos a word of this length may carry and still match.
pub fn typo_budget(word: &str) -> usize {
    match word.chars().count() {
        n if n >= TWO_TYPOS_FROM => 2,
        n if n >= ONE_TYPO_FROM => 1,
        _ => 0,
    }
}

/// Whether the words of `phrase` appear in `haystack` side by side, in order.
fn contains_phrase<S: AsRef<str>>(haystack: &[S], phrase: &[String]) -> bool {
    if phrase.is_empty() {
        return true;
    }
    haystack
        .windows(phrase.len())
        .any(|window| window.iter().map(AsRef::as_ref).eq(phrase.iter().map(String::as_str)))
}
