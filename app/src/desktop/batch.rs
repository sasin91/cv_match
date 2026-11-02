//! Several job posts matched with one CV (match_service::match_jobs), ranked best first as
//! "Jobs ranked by match": a row per post (BatchRow, as the match page prints it), and a row
//! saying why for each post that couldn't be matched. A row opens its result under the list.

use iced::widget::{Column, button, column, row, text};
use iced::{Center, Element, Fill};

use super::matching::{Msg, tag_health};
use super::style;
use super::widgets::{self, ROW_PADDING};
use crate::matches::batch_row::{BatchRow, UNTITLED};
use crate::matches::match_result::MatchResult;
use crate::matches::match_score;
use crate::matches::match_service::MatchAnswer;

/// The longest label of a post that couldn't be matched, in characters.
const LABEL_LENGTH: usize = 80;
/// The score column's width.
const SCORE_WIDTH: f32 = 56.0;

/// A post of the batch.
pub struct Entry {
    /// The post as typed: its first line, or its link.
    label: String,
    outcome: Result<Matched, String>,
}

struct Matched {
    row: BatchRow,
    result: MatchResult,
    /// Why the match couldn't be saved, when it couldn't: it is shown, but gone when the app
    /// closes.
    save_error: Option<String>,
}

/// The batch's results, ranked.
pub struct Batch {
    entries: Vec<Entry>,
    /// The posts matched before with this CV, shown from the saved matches.
    before: usize,
    /// The posts matched and saved now.
    pub saved_now: usize,
    /// The row whose result is open under the list.
    pub open: Option<usize>,
}

impl Batch {
    /// The answers to the posts typed as typed, in their order, ranked best first; the posts
    /// that couldn't be matched last.
    pub fn new(typed: &[String], answers: &[Result<MatchAnswer, String>]) -> Self {
        let mut before = 0;
        let mut saved_now = 0;
        let mut entries: Vec<Entry> = typed
            .iter()
            .zip(answers)
            .map(|(typed, answer)| Entry {
                label: label_of(typed),
                outcome: match answer {
                    Ok(answer) => {
                        if answer.matched_before {
                            before += 1;
                        } else if answer.result.saved_id.is_some() {
                            saved_now += 1;
                        }
                        let url = answer
                            .fetched
                            .as_ref()
                            .map_or(answer.result.job_url.as_str(), |page| page.url.as_str());
                        Ok(Matched {
                            row: BatchRow::from(&answer.result, url),
                            result: answer.result.clone(),
                            save_error: answer.save_error.clone(),
                        })
                    }
                    Err(why) => Err(why.clone()),
                },
            })
            .collect();
        entries
            .sort_by_key(|entry| std::cmp::Reverse(entry.outcome.as_ref().map_or(-1, |matched| matched.row.percent)));
        Batch {
            entries,
            before,
            saved_now,
            open: None,
        }
    }

    /// "3 of 4 matched (1 matched before, shown from your saved matches), 1 couldn't be matched."
    pub fn note(&self) -> String {
        let matched = self.entries.iter().filter(|entry| entry.outcome.is_ok()).count();
        let failed = self.entries.len() - matched;
        let mut note = format!("{matched} of {} matched", self.entries.len());
        if self.before > 0 {
            note.push_str(&format!(
                " ({} matched before, shown from your saved matches)",
                self.before
            ));
        }
        if failed > 0 {
            note.push_str(&format!(", {failed} couldn't be matched"));
        }
        let unsaved = self
            .entries
            .iter()
            .filter(|entry| entry.outcome.as_ref().is_ok_and(|matched| matched.save_error.is_some()))
            .count();
        if unsaved > 0 {
            let were = if unsaved == 1 { "wasn't" } else { "weren't" };
            note.push_str(&format!(", {unsaved} {were} saved"));
        }
        note.push('.');
        note
    }

    /// The result of row index, to open under the list.
    pub fn result(&self, index: usize) -> Option<&MatchResult> {
        self.entries
            .get(index)?
            .outcome
            .as_ref()
            .ok()
            .map(|matched| &matched.result)
    }
}

/// A post's label: its first line with text, cut to LABEL_LENGTH.
fn label_of(typed: &str) -> String {
    let line = typed
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(UNTITLED);
    if line.chars().count() <= LABEL_LENGTH {
        return line.to_string();
    }
    let cut: String = line.chars().take(LABEL_LENGTH - 1).collect();
    format!("{cut}…")
}

pub fn view(batch: &Batch) -> Element<'_, Msg> {
    let rows = widgets::rows(
        batch
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| entry_row(index, entry, batch.open == Some(index))),
    );
    column![
        widgets::section_title("Jobs ranked by match"),
        widgets::caption(batch.note()),
        widgets::card(rows),
    ]
    .spacing(10)
    .into()
}

fn entry_row(index: usize, entry: &Entry, open: bool) -> Element<'_, Msg> {
    let matched = match &entry.outcome {
        Ok(matched) => matched,
        Err(why) => {
            return row![
                text("–")
                    .size(style::SECTION_SIZE)
                    .style(style::secondary)
                    .width(SCORE_WIDTH),
                column![
                    text(entry.label.as_str())
                        .font(style::semibold())
                        .size(style::BODY_SIZE),
                    widgets::errors_line(why.as_str()),
                ]
                .spacing(2)
                .width(Fill),
            ]
            .spacing(12)
            .align_y(Center)
            .padding(ROW_PADDING)
            .into();
        }
    };
    let row_ = &matched.row;
    let mut about = vec![match_score::tag_name(&row_.tag).to_string()];
    if !row_.host.is_empty() {
        about.push(row_.host.clone());
    }
    if !row_.matched_at.is_empty() {
        about.push(format!("matched before, {}", row_.matched_at));
    }
    if let Some(chip) = &row_.apply_chip {
        about.push(chip.label.to_string());
    }
    let mut lines = Column::new()
        .spacing(2)
        .width(Fill)
        .push(
            text(row_.heading.as_str())
                .font(style::semibold())
                .size(style::BODY_SIZE),
        )
        .push(widgets::caption(about.join(" · ")));
    if !row_.line.is_empty() {
        lines = lines.push(widgets::caption(row_.line.as_str()));
    }
    if let Some(why) = &matched.save_error {
        lines = lines.push(widgets::errors_line(format!("Not saved: {why}")));
    }
    let health = tag_health(&row_.tag);
    button(
        row![
            text(format!("{}%", row_.percent))
                .font(style::semibold())
                .size(style::SECTION_SIZE)
                .style(style::health_text(health))
                .width(SCORE_WIDTH),
            lines,
            text(if open { "⌄" } else { "›" })
                .size(style::SECTION_SIZE)
                .style(style::secondary),
        ]
        .spacing(12)
        .align_y(Center),
    )
    .padding(ROW_PADDING)
    .width(Fill)
    .style(style::row_button)
    .on_press(Msg::OpenRanked(index))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matches::match_result::LayaLine;
    use crate::matches::match_score::MatchScore;

    fn answer(percent: i64, saved_id: Option<i64>, matched_before: bool) -> Result<MatchAnswer, String> {
        Ok(MatchAnswer {
            fetched: None,
            read_cv: None,
            result: MatchResult {
                saved_id,
                score: MatchScore::of(percent as f64 / 100.0, "good"),
                heading: format!("Job at {percent}"),
                parts: String::new(),
                saved_at: String::new(),
                reject: None,
                location: None,
                apply: None,
                filters: Vec::new(),
                sections: Vec::new(),
                soft_skills: Vec::new(),
                laya: LayaLine::default(),
                laya_input: String::new(),
                documents: None,
                job_url: "https://www.jobindex.dk/job/1".to_string(),
                job_text: String::new(),
                cv_found: None,
            },
            matched_before,
            save_error: None,
        })
    }

    #[test]
    fn the_posts_are_ranked_best_first_and_the_failed_ones_last() {
        let typed: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let answers = [
            answer(40, Some(1), false),
            Err("The link couldn't be read.".to_string()),
            answer(81, Some(2), true),
            answer(60, None, false).map(|answer| MatchAnswer {
                save_error: Some("The database is locked.".to_string()),
                ..answer
            }),
        ];
        let batch = Batch::new(&typed, &answers);
        let ranked: Vec<_> = batch
            .entries
            .iter()
            .map(|entry| entry.outcome.as_ref().map(|matched| matched.row.percent).ok())
            .collect();
        assert_eq!(ranked, [Some(81), Some(60), Some(40), None]);
        assert_eq!(batch.entries[3].label, "b");
        assert_eq!(batch.saved_now, 1);
        assert_eq!(
            batch.note(),
            "3 of 4 matched (1 matched before, shown from your saved matches), 1 couldn't be matched, 1 wasn't saved."
        );
        assert_eq!(batch.result(0).map(|result| result.saved_id), Some(Some(2)));
    }

    #[test]
    fn a_failed_posts_label_is_its_first_line_cut_short() {
        assert_eq!(label_of("\n  Lagermedarbejder  \nmore"), "Lagermedarbejder");
        assert_eq!(label_of("   "), UNTITLED);
        let long = "x".repeat(100);
        assert_eq!(label_of(&long).chars().count(), LABEL_LENGTH);
    }
}
