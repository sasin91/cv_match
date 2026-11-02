//! A job post's career level (student job, internship, graduate, junior, senior, lead) against
//! the candidate's years of work.
//!
//! The level is read with the post: the model gives it in extract_job's `level`, and where it
//! gives none (or there is no model) the post's title words decide ("Studentermedhjælper",
//! "Seniorudvikler"). A clear mismatch, such as a student job for someone with ten years of
//! work, caps the score like a missed location (matcher::DISQUALIFIED_INDEX), so the post ranks
//! below real fits and isn't tailored.

use std::sync::LazyLock;

use regex::Regex;

use super::Job;
use super::matcher::Verdict;

pub const STUDENT: &str = "student";
pub const INTERN: &str = "intern";
pub const GRADUATE: &str = "graduate";
pub const JUNIOR: &str = "junior";
pub const SENIOR: &str = "senior";
pub const LEAD: &str = "lead";

/// The levels a post can state, from the earliest in a career. A post that states none (most
/// posts, and "mid-level" ones) has level "".
pub const LEVELS: [&str; 6] = [STUDENT, INTERN, GRADUATE, JUNIOR, SENIOR, LEAD];

/// Most years of work a candidate can have and still fit a student job or an internship: more
/// than this is someone past their studies.
const STUDENT_MOST_YEARS: i64 = 2;
/// Most years of work a candidate can have and still fit a graduate position.
const GRADUATE_MOST_YEARS: i64 = 2;
/// Most years of work a candidate can have and still fit a junior position.
const JUNIOR_MOST_YEARS: i64 = 4;
/// Fewest years of work a senior position takes.
const SENIOR_LEAST_YEARS: i64 = 5;
/// Fewest years of work a lead position takes.
const LEAD_LEAST_YEARS: i64 = 7;

/// A level as the result shows it.
pub fn label(level: &str) -> &'static str {
    match level {
        STUDENT => "Student job",
        INTERN => "Internship",
        GRADUATE => "Graduate position",
        JUNIOR => "Junior position",
        SENIOR => "Senior position",
        LEAD => "Lead position",
        _ => "",
    }
}

/// What a level asks of the candidate's years: at most some (an early-career post), or at least
/// some (a senior one).
enum Years {
    AtMost(i64),
    AtLeast(i64),
}

fn years_of(level: &str) -> Option<Years> {
    match level {
        STUDENT | INTERN => Some(Years::AtMost(STUDENT_MOST_YEARS)),
        GRADUATE => Some(Years::AtMost(GRADUATE_MOST_YEARS)),
        JUNIOR => Some(Years::AtMost(JUNIOR_MOST_YEARS)),
        SENIOR => Some(Years::AtLeast(SENIOR_LEAST_YEARS)),
        LEAD => Some(Years::AtLeast(LEAD_LEAST_YEARS)),
        _ => None,
    }
}

/// The post's level as one of LEVELS, or "" when it states none: the model's, when it gave a
/// known one, otherwise what the title words say.
pub fn level_of(job: &Job) -> String {
    let given = job.level.trim().to_lowercase();
    if LEVELS.contains(&given.as_str()) {
        return given;
    }
    title_level(&[&job.job_title, &job.job_title_en, &job.heading]).to_string()
}

/// The job with its level read (level_of), for every reader to hand on the same way.
pub fn with_level(mut job: Job) -> Job {
    job.level = level_of(&job);
    job
}

/// Title words for each level. Danish compounds put the level first ("Seniorudvikler",
/// "Studentermedhjælper") or last ("Kontorelev"), so the words aren't cut at both ends. "Intern"
/// alone is left out: in Danish it is "internal" ("Intern konsulent").
static TITLE_WORDS: LazyLock<[(&str, Regex); 6]> = LazyLock::new(|| {
    let words = |pattern: &str| Regex::new(&format!("(?i){pattern}")).expect("level title words");
    [
        (
            STUDENT,
            words(r"\b(studie|studenter|student|studerende)|\b(lærling|apprentice)|elev(er)?\b|\bpart-time student"),
        ),
        (INTERN, words(r"\b(praktik|internship|working student)")),
        (GRADUATE, words(r"\b(graduate|nyuddannet|dimittend|trainee)")),
        (JUNIOR, words(r"\b(junior|jr\.)")),
        (SENIOR, words(r"\b(senior|sr\.)")),
        (LEAD, words(r"\b(lead|principal|head of|teamleder|team lead|tech lead)")),
    ]
});

/// The one level the title words name, or "" when they name none or several ("Junior/Senior
/// developer" is no level).
fn title_level(titles: &[&str]) -> &'static str {
    let mut found: Option<&'static str> = None;
    for (level, words) in TITLE_WORDS.iter() {
        if titles.iter().any(|title| words.is_match(title)) {
            if found.is_some() {
                return "";
            }
            found = Some(level);
        }
    }
    found.unwrap_or("")
}

/// The rule's verdict on a post's level for a candidate with have years of work. An
/// early-career post is missing for someone past it; a senior one is missing for someone with
/// under half its years, partial below them. A CV that shows no years can't be weighed: partial.
pub fn verdict(level: &str, have: i64) -> Verdict {
    let label = label(level);
    let evidence = format!("{have} years");
    let (verdict, reason) = match years_of(level) {
        None => ("met", format!("{label}.")),
        Some(_) if have <= 0 => (
            "partial",
            format!("{label}; the CV shows no years of work to weigh it by."),
        ),
        Some(Years::AtMost(most)) if have > most => (
            "missing",
            format!("{label}, for up to {most} years of work; the CV shows {have}."),
        ),
        Some(Years::AtMost(most)) => (
            "met",
            format!("{label}, for up to {most} years of work; the CV shows {have}."),
        ),
        Some(Years::AtLeast(least)) if have >= least => (
            "met",
            format!("{label}, for {least} or more years of work; the CV shows {have}."),
        ),
        Some(Years::AtLeast(least)) if have as f64 >= least as f64 / 2.0 => (
            "partial",
            format!("{label}, for {least} or more years of work; the CV shows {have}."),
        ),
        Some(Years::AtLeast(least)) => (
            "missing",
            format!("{label}, for {least} or more years of work; the CV shows {have}."),
        ),
    };
    Verdict {
        verdict: verdict.into(),
        by: "rule".into(),
        method: "years".into(),
        evidence,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titled(title: &str) -> Job {
        Job {
            job_title: title.into(),
            ..Job::default()
        }
    }

    #[test]
    fn title_words_name_the_level() {
        assert_eq!(level_of(&titled("Studentermedhjælper til IT-support")), STUDENT);
        assert_eq!(level_of(&titled("Studiejob som udvikler")), STUDENT);
        assert_eq!(level_of(&titled("Kontorelev")), STUDENT);
        assert_eq!(level_of(&titled("Praktikant i marketing")), INTERN);
        assert_eq!(level_of(&titled("Seniorudvikler, PHP")), SENIOR);
        assert_eq!(level_of(&titled("Junior Laravel developer")), JUNIOR);
        assert_eq!(level_of(&titled("Tech Lead")), LEAD);
        assert_eq!(level_of(&titled("PHP-udvikler")), "");
        assert_eq!(level_of(&titled("Relevant udvikler")), "");
        assert_eq!(level_of(&titled("Junior/Senior developer")), "");
    }

    #[test]
    fn the_models_level_comes_first() {
        let job = Job {
            level: "Senior".into(),
            ..titled("Studentermedhjælper")
        };
        assert_eq!(level_of(&job), SENIOR);
        let unknown = Job {
            level: "mid".into(),
            ..titled("Studentermedhjælper")
        };
        assert_eq!(level_of(&unknown), STUDENT);
    }

    #[test]
    fn a_student_job_is_missing_for_ten_years_of_work() {
        assert_eq!(verdict(STUDENT, 10).verdict, "missing");
        assert_eq!(verdict(STUDENT, 1).verdict, "met");
        assert_eq!(verdict(JUNIOR, 10).verdict, "missing");
        assert_eq!(verdict(SENIOR, 10).verdict, "met");
        assert_eq!(verdict(SENIOR, 3).verdict, "partial");
        assert_eq!(verdict(LEAD, 2).verdict, "missing");
        assert_eq!(verdict(SENIOR, 0).verdict, "partial");
    }
}
