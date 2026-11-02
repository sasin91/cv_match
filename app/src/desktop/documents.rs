//! A saved match's application and résumé in the window: the notes for the AI, and a box per
//! document with Write, Copy and Save as PDF, its text to edit, the keywords a tailored résumé
//! took over, its earlier versions, and the tailored résumé's own score next to the CV's. The
//! writing itself is match_service::write_document; this is its state and its look.

use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{Column, button, column, container, row, text, text_editor};
use iced::{Center, Element, Fill};

use super::GAP;
use super::style;
use super::widgets::{self, CARD_PADDING};
use crate::context::Context;
use crate::dashboard::Health;
use crate::matches::document_box::{DocumentBox, RESUME};
use crate::matches::match_documents::MatchDocuments;
use crate::matches::match_service::{self, WriteAnswer};
use crate::matches::resume_comparison::ResumeComparison;
use crate::service_error::{ErrorKind, ServiceError};

/// The notes box's height.
const NOTES_HEIGHT: f32 = 80.0;
/// A document's text box's height.
const TEXT_HEIGHT: f32 = 320.0;

/// A saved match's documents, as shown.
pub struct Docs {
    /// None when the match wasn't saved: error says why.
    pub id: Option<i64>,
    error: String,
    intro: &'static str,
    notes_label: &'static str,
    notes_hint: &'static str,
    notes_max: usize,
    notes: text_editor::Content,
    boxes: Vec<DocBox>,
    /// The tailored résumé's own match next to the CV's, or why it couldn't be made.
    comparison: Option<Result<ResumeComparison, String>>,
    /// The kind whose Write waits for "Write it again" (it replaces a text, edits included).
    confirming: Option<&'static str>,
}

/// One document's box: what was written, and its text as edited here.
struct DocBox {
    doc: DocumentBox,
    text: text_editor::Content,
    /// Its earlier versions are shown.
    versions_open: bool,
}

impl DocBox {
    fn new(doc: DocumentBox) -> Self {
        DocBox {
            text: text_editor::Content::with_text(&doc.text),
            doc,
            versions_open: false,
        }
    }

    /// The text as it is in the box now, edits included.
    fn edited(&self) -> String {
        self.text.text().trim().to_string()
    }

    /// The box holds a text the user changed.
    fn is_edited(&self) -> bool {
        self.edited() != self.doc.text.trim()
    }
}

impl Docs {
    pub fn new(documents: &MatchDocuments) -> Self {
        Docs {
            id: documents.id,
            error: documents.error.clone(),
            intro: documents.intro,
            notes_label: documents.notes_label,
            notes_hint: documents.notes_hint,
            notes_max: documents.notes_max,
            notes: text_editor::Content::new(),
            boxes: documents.boxes.iter().cloned().map(DocBox::new).collect(),
            comparison: None,
            confirming: None,
        }
    }

    fn box_of(&self, kind: &str) -> Option<&DocBox> {
        self.boxes.iter().find(|doc| doc.doc.kind == kind)
    }

    fn box_of_mut(&mut self, kind: &str) -> Option<&mut DocBox> {
        self.boxes.iter_mut().find(|doc| doc.doc.kind == kind)
    }

    /// Whether each document can be written now, as boxes (read again) say; the texts stay.
    pub fn availability(&mut self, boxes: &[DocumentBox]) {
        for now in boxes {
            if let Some(doc) = self.box_of_mut(now.kind) {
                doc.doc.off = now.off;
                doc.doc.off_note = now.off_note.clone();
            }
        }
    }

    /// The notes for the AI, cut to the panel's most.
    pub fn notes(&self) -> String {
        self.notes.text().trim().chars().take(self.notes_max).collect()
    }

    /// The text of kind, or of its earlier version document_id, as shown.
    pub fn text_of(&self, kind: &str, document_id: Option<i64>) -> Option<String> {
        let doc = self.box_of(kind)?;
        match document_id {
            None => Some(doc.edited()),
            Some(id) => doc
                .doc
                .earlier
                .iter()
                .find(|version| version.id == id)
                .map(|version| version.text.clone()),
        }
    }

    /// Whether Write of kind should ask first: it would replace a text.
    pub fn asks_first(&self, kind: &str) -> bool {
        self.confirming != Some(kind) && self.box_of(kind).is_some_and(|doc| !doc.edited().is_empty())
    }

    /// What writing answered: the boxes as they are now, and the résumé's comparison.
    pub fn written(&mut self, answer: &WriteAnswer) {
        let mut before = std::mem::take(&mut self.boxes);
        self.boxes = answer
            .boxes
            .iter()
            .cloned()
            .map(|mut doc| {
                let kept = before
                    .iter_mut()
                    .find(|old| old.doc.kind == doc.kind && old.doc.written_at == doc.written_at);
                match kept {
                    // Not written again: its edits stay, as only the box holds them, and so do
                    // the keywords its writing found, as reading it back leaves them out.
                    Some(old) => {
                        if doc.keywords.is_none() && doc.left_out.is_empty() {
                            doc.keywords = old.doc.keywords.take();
                            doc.left_out = std::mem::take(&mut old.doc.left_out);
                        }
                        DocBox {
                            text: std::mem::take(&mut old.text),
                            versions_open: old.versions_open,
                            doc,
                        }
                    }
                    None => DocBox::new(doc),
                }
            })
            .collect();
        if let Some(comparison) = &answer.comparison {
            self.comparison = Some(comparison.clone());
        }
        self.confirming = None;
    }

    pub fn update(&mut self, msg: DocMsg) {
        match msg {
            DocMsg::Notes(action) => self.notes.perform(action),
            DocMsg::Text(kind, action) => {
                if let Some(doc) = self.box_of_mut(kind) {
                    doc.text.perform(action);
                }
            }
            DocMsg::ToggleVersions(kind) => {
                if let Some(doc) = self.box_of_mut(kind) {
                    doc.versions_open = !doc.versions_open;
                }
            }
            DocMsg::Write(kind) => self.confirming = Some(kind),
            DocMsg::CancelWrite => self.confirming = None,
            // The screen's (matching): they run work or go somewhere.
            DocMsg::Copy(..) | DocMsg::SavePdf(..) | DocMsg::Open(_) | DocMsg::OpenLanguageModels => {}
        }
    }
}

#[derive(Debug, Clone)]
pub enum DocMsg {
    Notes(text_editor::Action),
    Text(&'static str, text_editor::Action),
    ToggleVersions(&'static str),
    /// Write kind; asks first when it replaces a text.
    Write(&'static str),
    CancelWrite,
    /// Copy kind's text, or its earlier version's.
    Copy(&'static str, Option<i64>),
    /// Save kind's text, or its earlier version's, as a PDF.
    SavePdf(&'static str, Option<i64>),
    /// Open a saved match: the tailored résumé's.
    Open(i64),
    OpenLanguageModels,
}

/// Saved match id's kind as a PDF (match_service::pdf), saved where the user picks; None when
/// the dialog was closed.
pub async fn save_pdf(
    context: Arc<Context>,
    id: i64,
    kind: &'static str,
    edited: String,
    document_id: Option<i64>,
) -> Result<Option<PathBuf>, ServiceError> {
    let made = match_service::pdf(&context, id, kind, &edited, document_id).await?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Save the PDF")
        .set_file_name(&made.name)
        .add_filter("PDF", &["pdf"])
        .save_file()
        .await
    else {
        return Ok(None);
    };
    file.write(&made.bytes).await.map_err(|e| {
        crate::log_line!("match/pdf: {}: {e}", file.path().display());
        ServiceError::new(ErrorKind::Invalid, format!("The PDF couldn't be saved: {e}"))
    })?;
    Ok(Some(file.path().to_path_buf()))
}

pub fn view(docs: &Docs, busy: bool) -> Element<'_, DocMsg> {
    if !docs.error.is_empty() {
        return widgets::message_bar(Health::Attention, docs.error.as_str());
    }
    let mut panel = Column::new()
        .spacing(GAP)
        .push(widgets::caption(docs.intro))
        .push(widgets::field(
            docs.notes_label,
            text_editor(&docs.notes)
                .placeholder(docs.notes_hint)
                .on_action(DocMsg::Notes)
                .size(style::BODY_SIZE)
                .padding(8)
                .height(NOTES_HEIGHT)
                .style(style::text_area)
                .into(),
            None,
        ));
    for doc in &docs.boxes {
        panel = panel.push(widgets::divider()).push(box_view(docs, doc, busy));
    }
    container(panel).padding(CARD_PADDING).width(Fill).into()
}

fn box_view<'a>(docs: &'a Docs, doc: &'a DocBox, busy: bool) -> Element<'a, DocMsg> {
    let d = &doc.doc;
    let kind = d.kind;
    let mut body = column![text(d.title).font(style::semibold()).size(style::SECTION_SIZE)].spacing(8);
    if let Some(send_to) = &d.send_to {
        let subject = if send_to.subject.is_empty() {
            String::new()
        } else {
            format!(" with the subject “{}”", send_to.subject)
        };
        body = body.push(widgets::caption(format!(
            "The post asks for it by email: {}{subject}.",
            send_to.email
        )));
    }
    if d.off {
        body = body.push(
            row![
                text(format!("{} Pick a model for it under Language models.", d.off_note))
                    .size(style::CAPTION_SIZE)
                    .style(style::secondary)
                    .width(Fill),
                button(text("Language models").size(style::BODY_SIZE))
                    .style(style::standard_button)
                    .on_press(DocMsg::OpenLanguageModels),
            ]
            .spacing(12)
            .align_y(Center),
        );
    }
    let written = !d.text.is_empty();
    let mut actions = row![
        button(text(d.write).size(style::BODY_SIZE))
            .style(style::primary_button)
            .on_press_maybe((!busy && !d.off).then_some(DocMsg::Write(kind)))
    ]
    .spacing(8)
    .align_y(Center);
    if written {
        actions = actions
            .push(
                button(text("Copy to clipboard").size(style::BODY_SIZE))
                    .style(style::standard_button)
                    .on_press(DocMsg::Copy(kind, None)),
            )
            .push(
                button(text("Save as PDF…").size(style::BODY_SIZE))
                    .style(style::standard_button)
                    .on_press_maybe((!busy).then_some(DocMsg::SavePdf(kind, None))),
            );
    }
    body = body.push(actions);
    if docs.confirming == Some(kind) {
        body = body.push(confirm_bar(kind, busy));
    }
    if let Some(keywords) = &d.keywords {
        body = body.push(text(keywords.summary.as_str()).size(style::CAPTION_SIZE));
    }
    if !d.left_out.is_empty() {
        body = body.push(widgets::caption(d.left_out.as_str()));
    }
    if kind == RESUME {
        match &docs.comparison {
            Some(Ok(comparison)) => {
                body = body.push(
                    row![
                        text(format!("{} {}", comparison.line, comparison.saved))
                            .size(style::CAPTION_SIZE)
                            .width(Fill),
                        button(text(comparison.open).size(style::BODY_SIZE))
                            .style(style::standard_button)
                            .on_press_maybe((!busy).then_some(DocMsg::Open(comparison.open_id))),
                    ]
                    .spacing(12)
                    .align_y(Center),
                );
            }
            Some(Err(why)) => {
                body = body.push(
                    text(format!(
                        "The tailored résumé couldn't be matched against the post: {why}"
                    ))
                    .size(style::CAPTION_SIZE)
                    .style(style::health_text(Health::Attention)),
                );
            }
            None => {}
        }
    }
    if written {
        body = body.push(
            text_editor(&doc.text)
                .on_action(move |action| DocMsg::Text(kind, action))
                .size(style::BODY_SIZE)
                .padding(8)
                .height(TEXT_HEIGHT)
                .style(style::text_area),
        );
        // A résumé written as fields is laid out from them, so its PDF hasn't the box's edits.
        if d.structured && doc.is_edited() {
            body = body.push(widgets::caption(
                "Your edits aren't in the PDF: it's laid out from the résumé as written. Copy has them. Add them to the notes and write it again to have them in the PDF.",
            ));
        }
    }
    if !d.earlier.is_empty() {
        body = body.push(
            button(
                text(format!(
                    "{} {} ({})",
                    if doc.versions_open { "▾" } else { "▸" },
                    d.earlier_heading,
                    d.earlier.len()
                ))
                .size(style::BODY_SIZE),
            )
            .padding([4.0, 0.0])
            .style(style::subtle_button(false))
            .on_press(DocMsg::ToggleVersions(kind)),
        );
        if doc.versions_open {
            for version in &d.earlier {
                body = body.push(version_view(kind, version, busy));
            }
        }
    }
    body.into()
}

/// "Write it again?" under the box's buttons.
fn confirm_bar(kind: &'static str, busy: bool) -> Element<'static, DocMsg> {
    container(
        column![
            text("The new one takes the place of the text below, your edits included. The one written before stays under Earlier versions.")
                .size(style::CAPTION_SIZE),
            row![
                button(text("Write it again").size(style::BODY_SIZE))
                    .style(style::primary_button)
                    .on_press_maybe((!busy).then_some(DocMsg::Write(kind))),
                button(text("Cancel").size(style::BODY_SIZE))
                    .style(style::standard_button)
                    .on_press(DocMsg::CancelWrite),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
    .padding([10.0, 14.0])
    .width(Fill)
    .style(style::warning_bar)
    .into()
}

/// An earlier version: when and by what it was written, its notes, Copy and Save as PDF, and
/// its text.
fn version_view<'a>(
    kind: &'static str,
    version: &'a crate::matches::document_box::EarlierVersion,
    busy: bool,
) -> Element<'a, DocMsg> {
    let mut line = format!("Written {}", day_and_time(&version.written_at));
    if !version.written_by.is_empty() {
        line.push_str(&format!(" by {}", version.written_by));
    }
    if !version.notes.is_empty() {
        line.push_str(&format!(", with the notes “{}”", version.notes));
    }
    line.push('.');
    let id = version.id;
    container(
        column![
            row![
                text(line).size(style::CAPTION_SIZE).style(style::secondary).width(Fill),
                button(text("Copy").size(style::BODY_SIZE))
                    .style(style::standard_button)
                    .on_press(DocMsg::Copy(kind, Some(id))),
                button(text("Save as PDF…").size(style::BODY_SIZE))
                    .style(style::standard_button)
                    .on_press_maybe((!busy).then_some(DocMsg::SavePdf(kind, Some(id)))),
            ]
            .spacing(8)
            .align_y(Center),
            text(version.text.as_str()).size(style::CAPTION_SIZE),
        ]
        .spacing(8),
    )
    .padding([10.0, 14.0])
    .width(Fill)
    .style(style::card)
    .into()
}

/// "2026-11-02 09:10" of an ISO 8601 time, in this computer's time zone; as it is when it can't
/// be read.
pub(super) fn day_and_time(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso).map_or_else(
        |_| iso.to_string(),
        |time| time.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::matches::document_box::{APPLICATION, Written};
    use crate::matches::match_documents::Uses;

    /// A saved match's panel with the application written as text.
    fn docs(application: &str) -> Docs {
        let written = Written {
            text: application.to_string(),
            ..Written::default()
        };
        let uses = Uses {
            resume: true,
            application: true,
            job_evaluation: true,
        };
        Docs::new(&MatchDocuments::from(
            Some(21),
            "",
            &(written, Written::default()),
            uses,
            None,
        ))
    }

    #[test]
    fn writing_over_a_text_asks_first_and_then_writes() {
        let mut written = docs("Kære Firma");
        assert!(written.asks_first(APPLICATION));
        written.update(DocMsg::Write(APPLICATION));
        assert!(!written.asks_first(APPLICATION));
        written.update(DocMsg::CancelWrite);
        assert!(written.asks_first(APPLICATION));
        assert!(!docs("").asks_first(APPLICATION));
    }

    #[test]
    fn copy_and_pdf_take_the_text_as_edited() {
        let mut written = docs("Kære Firma");
        written.update(DocMsg::Text(
            APPLICATION,
            text_editor::Action::Move(text_editor::Motion::DocumentEnd),
        ));
        written.update(DocMsg::Text(
            APPLICATION,
            text_editor::Action::Edit(text_editor::Edit::Insert('!')),
        ));
        assert_eq!(written.text_of(APPLICATION, None).as_deref(), Some("Kære Firma!"));
    }

    #[test]
    fn writing_one_document_keeps_the_edits_of_the_other() {
        let mut panel = docs("Kære Firma");
        panel.update(DocMsg::Text(
            APPLICATION,
            text_editor::Action::Edit(text_editor::Edit::Insert('!')),
        ));
        let mut boxes: Vec<DocumentBox> = panel.boxes.iter().map(|doc| doc.doc.clone()).collect();
        // The résumé written now; the application as it was.
        boxes[1].text = "Résumé".to_string();
        boxes[1].written_at = "1762000000".to_string();
        panel.written(&WriteAnswer {
            boxes,
            comparison: None,
            done: "Résumé tailored",
            problem: None,
        });
        assert_eq!(panel.text_of(APPLICATION, None).as_deref(), Some("!Kære Firma"));
        assert_eq!(panel.text_of(RESUME, None).as_deref(), Some("Résumé"));
    }

    #[test]
    fn writing_the_application_keeps_the_keywords_of_the_resume() {
        let mut panel = docs("Kære Firma");
        panel.boxes[1].doc.left_out = "Left out, since your CV doesn't show it: Go".to_string();
        let mut boxes: Vec<DocumentBox> = panel.boxes.iter().map(|doc| doc.doc.clone()).collect();
        // The application written now; the résumé read back, which leaves its keywords out.
        boxes[0].written_at = "1762000000".to_string();
        boxes[1].left_out = String::new();
        panel.written(&WriteAnswer {
            boxes,
            comparison: None,
            done: "Application written",
            problem: None,
        });
        assert_eq!(
            panel.boxes[1].doc.left_out,
            "Left out, since your CV doesn't show it: Go"
        );
    }

    #[test]
    fn a_key_set_while_away_turns_writing_on_and_keeps_the_edits() {
        let mut panel = docs("Kære Firma");
        panel.boxes[0].doc.off = true;
        panel.update(DocMsg::Text(
            APPLICATION,
            text_editor::Action::Edit(text_editor::Edit::Insert('!')),
        ));
        let mut now: Vec<DocumentBox> = panel.boxes.iter().map(|doc| doc.doc.clone()).collect();
        now[0].off = false;
        panel.availability(&now);
        assert!(!panel.boxes[0].doc.off);
        assert_eq!(panel.text_of(APPLICATION, None).as_deref(), Some("!Kære Firma"));
    }

    #[test]
    fn a_time_is_shown_to_the_minute() {
        assert_eq!(
            day_and_time("2026-11-02T08:10:52+00:00").len(),
            "2026-11-02 08:10".len()
        );
        assert_eq!(day_and_time("soon"), "soon");
    }
}
