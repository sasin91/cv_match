//! A saved match's application and résumé in the window: the notes for the AI, and a box per
//! document with Write, Copy and Save as PDF, its text to edit, the keywords a tailored résumé
//! took over, its earlier versions, and the tailored résumé's own score next to the CV's. The
//! text is reviewed as a pull request is: highlight a passage to start a comment thread on it, or
//! ask the writer about it (match_service::ask_about), and Revise rewrites it on the threads, its
//! changes then marked against the version before. The writing itself is
//! match_service::write_document; this is its state and its look.

use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{Column, button, column, container, rich_text, row, span, text, text_editor, text_input};
use iced::{Center, Element, Fill};

use super::GAP;
use super::style;
use super::task_manager::{self, Working};
use super::widgets::{self, CARD_PADDING};
use crate::context::Context;
use crate::dashboard::Health;
use crate::matches::document_box::{DocumentBox, RESUME};
use crate::matches::document_question::{Answer, Question};
use crate::matches::document_writer::{By, Comment, Revision, Said};
use crate::matches::match_documents::MatchDocuments;
use crate::matches::match_service::{self, WriteAnswer};
use crate::matches::resume_comparison::ResumeComparison;
use crate::matches::text_changes::{self, Change};
use crate::service_error::{ErrorKind, ServiceError};

/// The notes box's height.
const NOTES_HEIGHT: f32 = 80.0;
/// A document's text box's height.
const TEXT_HEIGHT: f32 = 320.0;
/// Most characters of a highlighted passage shown above its comments; the rest is "…".
const PASSAGE_SHOWN: usize = 160;

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
    /// The kind being written or revised now.
    writing: Option<&'static str>,
}

/// One document's box: what was written, and its text as edited here.
struct DocBox {
    doc: DocumentBox,
    text: text_editor::Content,
    /// Its earlier versions are shown.
    versions_open: bool,
    /// The text highlighted in the box now, "" when none.
    highlighted: String,
    /// What's typed under the highlighted text: a comment or a question that starts a thread.
    draft: String,
    /// The comment threads the next revision is written on.
    threads: Vec<Thread>,
    /// The text the revision under way rewrites, to mark its changes once it is written.
    revising_from: Option<String>,
    /// What the last revision changed from the version before; None until one is written here.
    changes: Option<Vec<Change>>,
    /// The changes are shown.
    changes_open: bool,
}

/// A comment thread on a passage, as shown.
struct Thread {
    comment: Comment,
    /// What's typed as the next reply.
    reply: String,
    /// A question in it waits for the writer's answer.
    asking: bool,
    /// Why the last question got no answer.
    problem: Option<String>,
}

impl DocBox {
    fn new(doc: DocumentBox) -> Self {
        DocBox {
            text: text_editor::Content::with_text(&doc.text),
            doc,
            versions_open: false,
            highlighted: String::new(),
            draft: String::new(),
            threads: Vec::new(),
            revising_from: None,
            changes: None,
            changes_open: false,
        }
    }

    /// A box written anew in place of old: a revision's changes are marked against the text it
    /// rewrote and its threads are done; after any other writing the threads stay.
    fn replacing(doc: DocumentBox, old: &mut DocBox) -> Self {
        let mut written = DocBox::new(doc);
        match old.revising_from.take() {
            Some(before) => {
                written.changes = Some(text_changes::changes(&before, &written.doc.text));
                written.changes_open = true;
            }
            None => written.threads = std::mem::take(&mut old.threads),
        }
        written
    }

    /// The candidate starts a thread on the highlighted text with what's typed; its index.
    fn start_thread(&mut self) -> Option<usize> {
        let passage = self.highlighted.trim();
        let said = self.draft.trim();
        if passage.is_empty() || said.is_empty() {
            return None;
        }
        self.threads.push(Thread {
            comment: Comment {
                passage: passage.to_string(),
                said: vec![candidate(said)],
            },
            reply: String::new(),
            asking: false,
            problem: None,
        });
        self.draft.clear();
        Some(self.threads.len() - 1)
    }

    /// The candidate's reply in thread index, as typed; false when nothing is.
    fn reply(&mut self, index: usize) -> bool {
        let Some(thread) = self.threads.get_mut(index) else {
            return false;
        };
        let said = thread.reply.trim();
        if said.is_empty() {
            return false;
        }
        thread.comment.said.push(candidate(said));
        thread.reply.clear();
        true
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
            writing: None,
        }
    }

    /// kind is being written now (None: nothing is).
    pub fn set_writing(&mut self, kind: Option<&'static str>) {
        self.writing = kind;
    }

    /// The box the window's work is for: the one being written, else one whose thread waits
    /// for an answer.
    fn working_on(&self) -> Option<&'static str> {
        self.writing.or_else(|| {
            self.boxes
                .iter()
                .find(|doc| doc.threads.iter().any(|thread| thread.asking))
                .map(|doc| doc.doc.kind)
        })
    }

    fn box_of(&self, kind: &str) -> Option<&DocBox> {
        self.boxes.iter().find(|doc| doc.doc.kind == kind)
    }

    fn box_of_mut(&mut self, kind: &str) -> Option<&mut DocBox> {
        self.boxes.iter_mut().find(|doc| doc.doc.kind == kind)
    }

    /// A thread on kind's highlighted text (ask: with what's typed as a question to the writer),
    /// or a reply in thread Some(index); the question to send when it asks. None when nothing
    /// was typed or highlighted, or the thread waits for an answer already.
    pub fn comment(&mut self, kind: &'static str, index: Option<usize>, ask: bool) -> Option<Question> {
        let notes = self.notes();
        let doc = self.box_of_mut(kind)?;
        let index = match index {
            Some(index) if doc.threads.get(index).is_some_and(|thread| !thread.asking) => {
                doc.reply(index).then_some(index)?
            }
            Some(_) => return None,
            None => doc.start_thread()?,
        };
        if !ask {
            return None;
        }
        let document = doc.edited();
        let thread = &mut doc.threads[index];
        thread.asking = true;
        thread.problem = None;
        Some(Question {
            kind,
            document,
            comment: thread.comment.clone(),
            notes,
        })
    }

    /// The writer's answer to the question asked in kind's thread on passage.
    pub fn answered(&mut self, kind: &str, passage: &str, answer: &Result<Answer, String>) {
        let Some(thread) = self.box_of_mut(kind).and_then(|doc| {
            doc.threads
                .iter_mut()
                .find(|thread| thread.asking && thread.comment.passage == passage)
        }) else {
            return;
        };
        thread.asking = false;
        match answer {
            Ok(answer) => thread.comment.said.push(Said {
                by: By::Writer,
                text: answer.said(),
            }),
            Err(why) => thread.problem = Some(why.clone()),
        }
    }

    /// kind's text as it is in its box, with its threads, to revise; kept to mark the changes.
    /// None without threads.
    pub fn revision(&mut self, kind: &str) -> Option<Revision> {
        let doc = self.box_of_mut(kind)?;
        if doc.threads.is_empty() {
            return None;
        }
        let previous = doc.edited();
        doc.revising_from = Some(previous.clone());
        Some(Revision {
            previous,
            comments: doc.threads.iter().map(|thread| thread.comment.clone()).collect(),
        })
    }

    /// The writing failed: no revision is under way any more, and the threads stay.
    pub fn not_written(&mut self) {
        self.writing = None;
        for doc in &mut self.boxes {
            doc.revising_from = None;
        }
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
                            highlighted: std::mem::take(&mut old.highlighted),
                            draft: std::mem::take(&mut old.draft),
                            threads: std::mem::take(&mut old.threads),
                            revising_from: old.revising_from.take(),
                            changes: old.changes.take(),
                            changes_open: old.changes_open,
                            doc,
                        }
                    }
                    None => match before.iter_mut().find(|old| old.doc.kind == doc.kind) {
                        Some(old) => DocBox::replacing(doc, old),
                        None => DocBox::new(doc),
                    },
                }
            })
            .collect();
        if let Some(comparison) = &answer.comparison {
            self.comparison = Some(comparison.clone());
        }
        self.confirming = None;
        self.writing = None;
    }

    pub fn update(&mut self, msg: DocMsg) {
        match msg {
            DocMsg::Notes(action) => self.notes.perform(action),
            DocMsg::Text(kind, action) => {
                if let Some(doc) = self.box_of_mut(kind) {
                    doc.text.perform(action);
                    // Kept as it is when the box loses focus, for the comment typed next.
                    doc.highlighted = doc.text.selection().unwrap_or_default();
                }
            }
            DocMsg::Draft(kind, draft) => {
                if let Some(doc) = self.box_of_mut(kind) {
                    doc.draft = draft;
                }
            }
            DocMsg::Reply(kind, index, reply) => {
                if let Some(thread) = self.box_of_mut(kind).and_then(|doc| doc.threads.get_mut(index)) {
                    thread.reply = reply;
                }
            }
            DocMsg::Comment(kind, index) => {
                self.comment(kind, index, false);
            }
            DocMsg::RemoveThread(kind, index) => {
                if let Some(doc) = self.box_of_mut(kind).filter(|doc| index < doc.threads.len()) {
                    doc.threads.remove(index);
                }
            }
            DocMsg::ToggleChanges(kind) => {
                if let Some(doc) = self.box_of_mut(kind) {
                    doc.changes_open = !doc.changes_open;
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
            DocMsg::Copy(..)
            | DocMsg::SavePdf(..)
            | DocMsg::Open(_)
            | DocMsg::OpenLanguageModels
            | DocMsg::Ask(..)
            | DocMsg::Revise(_)
            | DocMsg::Stop(_) => {}
        }
    }
}

#[derive(Debug, Clone)]
pub enum DocMsg {
    Notes(text_editor::Action),
    Text(&'static str, text_editor::Action),
    ToggleVersions(&'static str),
    /// What's typed under kind's highlighted text.
    Draft(&'static str, String),
    /// What's typed as the reply in kind's thread index.
    Reply(&'static str, usize, String),
    /// Start a thread on kind's highlighted text (None), or reply in thread Some(index), with
    /// what's typed.
    Comment(&'static str, Option<usize>),
    /// Comment, and ask the writer about it.
    Ask(&'static str, Option<usize>),
    RemoveThread(&'static str, usize),
    /// Rewrite kind on its threads.
    Revise(&'static str),
    ToggleChanges(&'static str),
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
    /// Stop the window's writing or question, from its working list.
    Stop(crate::task_manager::Task),
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

/// The panel; working: the window's writing and questions in the task manager, shown in the box
/// they are for.
pub fn view<'a>(docs: &'a Docs, busy: bool, working: &'a Working) -> Element<'a, DocMsg> {
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
        let working = (docs.working_on() == Some(doc.doc.kind)).then_some(working);
        panel = panel.push(widgets::divider()).push(box_view(docs, doc, busy, working));
    }
    container(panel).padding(CARD_PADDING).width(Fill).into()
}

fn box_view<'a>(docs: &'a Docs, doc: &'a DocBox, busy: bool, working: Option<&'a Working>) -> Element<'a, DocMsg> {
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
    if let Some(working) = working {
        body = body.push(task_manager::working_view(working, DocMsg::Stop));
    }
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
        body = body.push(review_view(doc, busy));
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

/// A written box's review: what the last revision changed, the highlighted text to comment on,
/// the comment threads, and Revise.
fn review_view(doc: &DocBox, busy: bool) -> Element<'_, DocMsg> {
    let kind = doc.doc.kind;
    let mut review = Column::new().spacing(8);
    if let Some(changes) = doc.changes.as_ref().filter(|changes| text_changes::any(changes)) {
        review = review.push(
            button(
                text(format!(
                    "{} Changes from the version before",
                    if doc.changes_open { "▾" } else { "▸" }
                ))
                .size(style::BODY_SIZE),
            )
            .padding([4.0, 0.0])
            .style(style::subtle_button(false))
            .on_press(DocMsg::ToggleChanges(kind)),
        );
        if doc.changes_open {
            review = review.push(changes_view(changes));
        }
    }
    let can_ask = !doc.doc.off;
    if doc.highlighted.trim().is_empty() {
        if doc.threads.is_empty() {
            review = review.push(widgets::caption(
                "Highlight part of the text to comment on it or ask the writer about it. Revise then rewrites it on your comments.",
            ));
        }
    } else {
        let typed = !doc.draft.trim().is_empty();
        review = review.push(
            container(
                column![
                    passage_line(&doc.highlighted),
                    row![
                        text_input("Comment, or ask where it came from…", &doc.draft)
                            .on_input(move |draft| DocMsg::Draft(kind, draft))
                            .on_submit_maybe(typed.then_some(DocMsg::Comment(kind, None)))
                            .size(style::BODY_SIZE)
                            .padding(8)
                            .style(style::text_field),
                        button(text("Comment").size(style::BODY_SIZE))
                            .style(style::standard_button)
                            .on_press_maybe(typed.then_some(DocMsg::Comment(kind, None))),
                        button(text("Ask").size(style::BODY_SIZE))
                            .style(style::standard_button)
                            .on_press_maybe((typed && can_ask).then_some(DocMsg::Ask(kind, None))),
                    ]
                    .spacing(8)
                    .align_y(Center),
                ]
                .spacing(8),
            )
            .padding([10.0, 14.0])
            .width(Fill)
            .style(style::card),
        );
    }
    for (index, thread) in doc.threads.iter().enumerate() {
        review = review.push(thread_view(kind, index, thread, can_ask));
    }
    if !doc.threads.is_empty() {
        let count = doc.threads.len();
        review = review.push(
            row![
                button(text(format!("Revise on {count} comment{}", if count == 1 { "" } else { "s" })).size(style::BODY_SIZE))
                    .style(style::primary_button)
                    .on_press_maybe((!busy && can_ask && !doc.threads.iter().any(|t| t.asking)).then_some(DocMsg::Revise(kind))),
                text("Rewrites the text above on your comments, edits included. The text before stays under Earlier versions, and the changes are marked.")
                    .size(style::CAPTION_SIZE)
                    .style(style::secondary)
                    .width(Fill),
            ]
            .spacing(12)
            .align_y(Center),
        );
    }
    review.into()
}

/// “The highlighted passage”, cut to PASSAGE_SHOWN characters.
fn passage_line(passage: &str) -> Element<'_, DocMsg> {
    let passage = passage.trim();
    let shown: String = passage.chars().take(PASSAGE_SHOWN).collect();
    let more = if shown.len() < passage.len() { "…" } else { "" };
    widgets::caption(format!("On “{shown}{more}”"))
}

/// One comment thread: its passage, what was said, a reply box, and Remove.
fn thread_view<'a>(kind: &'static str, index: usize, thread: &'a Thread, can_ask: bool) -> Element<'a, DocMsg> {
    let mut body = column![
        row![
            container(passage_line(&thread.comment.passage)).width(Fill),
            button(text("Remove").size(style::CAPTION_SIZE))
                .padding([2.0, 8.0])
                .style(style::subtle_button(false))
                .on_press(DocMsg::RemoveThread(kind, index)),
        ]
        .align_y(Center)
    ]
    .spacing(6);
    for said in &thread.comment.said {
        let who = match said.by {
            By::Candidate => "You",
            By::Writer => "Writer",
        };
        body = body.push(
            row![
                text(who).font(style::semibold()).size(style::BODY_SIZE),
                text(said.text.as_str()).size(style::BODY_SIZE).width(Fill),
            ]
            .spacing(8),
        );
    }
    if thread.asking {
        body = body.push(widgets::caption("The writer is answering…"));
    }
    if let Some(problem) = &thread.problem {
        body = body.push(
            text(format!("No answer: {problem}"))
                .size(style::CAPTION_SIZE)
                .style(style::health_text(Health::Attention)),
        );
    }
    let typed = !thread.reply.trim().is_empty() && !thread.asking;
    body = body.push(
        row![
            text_input("Reply…", &thread.reply)
                .on_input(move |reply| DocMsg::Reply(kind, index, reply))
                .on_submit_maybe(typed.then_some(DocMsg::Comment(kind, Some(index))))
                .size(style::BODY_SIZE)
                .padding(6)
                .style(style::text_field),
            button(text("Reply").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press_maybe(typed.then_some(DocMsg::Comment(kind, Some(index)))),
            button(text("Ask").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press_maybe((typed && can_ask).then_some(DocMsg::Ask(kind, Some(index)))),
        ]
        .spacing(8)
        .align_y(Center),
    );
    container(body)
        .padding([10.0, 14.0])
        .width(Fill)
        .style(style::card)
        .into()
}

/// The revised text with its changes marked: added words highlighted, removed ones struck
/// through.
fn changes_view(changes: &[Change]) -> Element<'_, DocMsg> {
    let spans: Vec<text::Span<'_, (), iced::Font>> = changes
        .iter()
        .map(|change| match change {
            Change::Same(words) => span(words.as_str()),
            Change::Added(words) => span(words.as_str()).background(style::ADDED_BACKGROUND),
            Change::Removed(words) => span(words.as_str()).color(style::REMOVED_TEXT).strikethrough(true),
        })
        .collect();
    container(rich_text(spans).size(style::BODY_SIZE))
        .padding([10.0, 14.0])
        .width(Fill)
        .style(style::card)
        .into()
}

/// A line the candidate said in a thread.
fn candidate(text: &str) -> Said {
    Said {
        by: By::Candidate,
        text: text.to_string(),
    }
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

    /// panel with "16 år" highlighted in its application.
    fn highlighted(panel: &mut Docs) {
        panel.boxes[0].highlighted = "16 år".to_string();
    }

    #[test]
    fn a_selection_in_the_box_is_kept_as_the_highlighted_text() {
        let mut panel = docs("Kære Firma");
        panel.update(DocMsg::Text(APPLICATION, text_editor::Action::SelectAll));
        assert_eq!(panel.boxes[0].highlighted, "Kære Firma");
        panel.update(DocMsg::Text(
            APPLICATION,
            text_editor::Action::Move(text_editor::Motion::DocumentEnd),
        ));
        assert_eq!(panel.boxes[0].highlighted, "");
    }

    #[test]
    fn a_question_starts_a_thread_on_the_highlighted_text_and_its_answer_joins_it() {
        let mut panel = docs("Jeg har arbejdet i 16 år.");
        highlighted(&mut panel);
        panel.update(DocMsg::Draft(APPLICATION, "Where does this come from?".to_string()));
        let question = panel.comment(APPLICATION, None, true).unwrap();
        assert_eq!(question.comment.passage, "16 år");
        assert_eq!(question.asked(), "Where does this come from?");
        assert_eq!(question.document, "Jeg har arbejdet i 16 år.");
        // A second question waits for the first answer.
        panel.update(DocMsg::Reply(APPLICATION, 0, "And?".to_string()));
        assert!(panel.comment(APPLICATION, Some(0), true).is_none());
        panel.answered(
            APPLICATION,
            "16 år",
            &Ok(Answer {
                answer: "Nothing in the CV backs it.".to_string(),
                change: "Say 9 years.".to_string(),
            }),
        );
        let thread = &panel.boxes[0].threads[0];
        assert!(!thread.asking);
        assert_eq!(thread.comment.said[1].by, By::Writer);
        assert_eq!(
            thread.comment.said[1].text,
            "Nothing in the CV backs it.\nSuggested change: Say 9 years."
        );
    }

    #[test]
    fn a_revision_marks_its_changes_and_ends_the_threads() {
        let mut panel = docs("Jeg har arbejdet i 16 år.");
        highlighted(&mut panel);
        panel.update(DocMsg::Draft(APPLICATION, "9 years, not 16.".to_string()));
        panel.update(DocMsg::Comment(APPLICATION, None));
        let revision = panel.revision(APPLICATION).unwrap();
        assert_eq!(revision.previous, "Jeg har arbejdet i 16 år.");
        assert_eq!(revision.comments[0].said[0].text, "9 years, not 16.");
        let mut boxes: Vec<DocumentBox> = panel.boxes.iter().map(|doc| doc.doc.clone()).collect();
        boxes[0].text = "Jeg har arbejdet i 9 år.".to_string();
        boxes[0].written_at = "1762000000".to_string();
        panel.written(&WriteAnswer {
            boxes,
            comparison: None,
            done: "Application written",
            problem: None,
        });
        let revised = &panel.boxes[0];
        assert!(revised.threads.is_empty());
        assert_eq!(
            revised.changes.as_deref(),
            Some(
                &[
                    Change::Same("Jeg har arbejdet i ".to_string()),
                    Change::Removed("16 ".to_string()),
                    Change::Added("9 ".to_string()),
                    Change::Same("år.".to_string()),
                ][..]
            )
        );
    }

    #[test]
    fn writing_it_again_without_revising_keeps_the_threads() {
        let mut panel = docs("Jeg har arbejdet i 16 år.");
        highlighted(&mut panel);
        panel.update(DocMsg::Draft(APPLICATION, "9 years, not 16.".to_string()));
        panel.update(DocMsg::Comment(APPLICATION, None));
        let mut boxes: Vec<DocumentBox> = panel.boxes.iter().map(|doc| doc.doc.clone()).collect();
        boxes[0].written_at = "1762000000".to_string();
        panel.written(&WriteAnswer {
            boxes,
            comparison: None,
            done: "Application written",
            problem: None,
        });
        assert_eq!(panel.boxes[0].threads.len(), 1);
        assert!(panel.boxes[0].changes.is_none());
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
