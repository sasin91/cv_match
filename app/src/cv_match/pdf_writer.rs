//! A job application, tailored résumé, CV or job post as a printable A4 PDF, ported from
//! modules/cv_match/Pdf_writer.php.
//!
//! A résumé tailored with fields (tailored_resume) is laid out from them by resume_pdf(). The
//! application, and a résumé written before résumés had fields, are plain text: the application
//! is paragraphs, the old résumé starts with the name and contact lines, then sections whose
//! headings stand on their own line, with "- " bullets. The CV as it was matched (kind cv) is
//! read like the old résumé, and the job post (kind job, for Jobnet's joblog) is its title,
//! company and link over its text.
//!
//! The PHP code wrote that as a small HTML page for dompdf. Here the same page is a tree
//! (pdf::page): html() prints it as the PHP code did, which the parity tests compare, and pdf()
//! lays it out itself (pdf::layout) in the same style. Nothing remote is loaded.

use std::sync::LazyLock;

use regex::Regex;

use super::tailored_resume::{self, ResumeEntry, TailoredResume};
use crate::pdf::page::{Block, BlockClass, Family, LineBreak, Node, Page, Paragraph, ParagraphClass};
use crate::pdf::{self, fonts};
use crate::php;

pub const KINDS: [&str; 4] = ["application", "resume", "cv", "job"];

/// What the downloads are called, by kind.
const FILE_NAMES: [(&str, &str); 4] = [
    ("application", "Application"),
    ("resume", "Resume"),
    ("cv", "CV"),
    ("job", "Job ad"),
];

/// The longest a download's name is before ".pdf", in characters.
const MAX_FILE_NAME: usize = 120;

/// The longest line read as a résumé's section heading, in characters.
const MAX_HEADING_LENGTH: usize = 40;

/// The most words a résumé's section heading has.
const MAX_HEADING_WORDS: usize = 4;

fn pattern(source: &str) -> Regex {
    Regex::new(source).unwrap_or_else(|e| panic!("{source}: {e}"))
}

/// A blank line (PHP's /\n\s*\n/, whose \s is ASCII only without /u).
static BLANK_LINE: LazyLock<Regex> = LazyLock::new(|| pattern(r"\n[\t\n\x0B\x0C\r ]*\n"));
/// A "- " bullet line, and its text.
static BULLET: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[-•*–]\s+(.*)$"));
/// A line that starts as a bullet, before trimming.
static STARTS_AS_BULLET: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\s*[-•*–]\s+"));
/// What a role or school line has and a heading doesn't: digits, a comma or a separator.
static NOT_IN_HEADING: LazyLock<Regex> = LazyLock::new(|| pattern(r"[\d,|·()]"));
/// The end of a sentence.
static SENTENCE_END: LazyLock<Regex> = LazyLock::new(|| pattern(r"[.!?;]$"));
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| pattern(r"\s+"));
/// Markdown bold, **text**.
static MARKDOWN_BOLD: LazyLock<Regex> = LazyLock::new(|| pattern(r"\*\*(.+?)\*\*"));
static TRAILING_SPACE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?m)[ \t]+$"));
/// Characters file systems refuse in a name.
static UNSAFE_IN_FILE_NAME: LazyLock<Regex> = LazyLock::new(|| pattern(r#"[\\/:*?"<>|\x00-\x1f]+"#));

/// The text as an A4 PDF document (the file's bytes).
///
/// `kind` is application, resume, cv or job; `text` the text as shown on the page; `title` the
/// PDF's title (its metadata), e.g. "Job application: Acme".
pub fn pdf(kind: &str, text: &str, title: &str) -> Result<Vec<u8>, String> {
    pdf::render(&text_page(kind, text, title))
}

/// A tailored résumé's fields as an A4 PDF, laid out like sasin91.xyz's cv.pdf: the name, title,
/// contact and link lines, the intro, then Experience, Skills, Languages and Education, each
/// entry as "Title · Organisation" over its dates and place, summary, bullets and note.
pub fn resume_pdf(resume: &TailoredResume, title: &str) -> Result<Vec<u8>, String> {
    pdf::render(&resume_fields_page(resume, title))
}

/// The résumé fields as the HTML page resume_pdf() renders.
pub fn resume_fields_html(resume: &TailoredResume, title: &str) -> String {
    resume_fields_page(resume, title).html()
}

/// The text as the HTML page pdf() renders. The layout is the one sasin91.xyz's cv.pdf uses:
/// Helvetica on A4 with 16mm side and 18mm top and bottom margins, black text, a 20pt name,
/// 11pt section headings, "role" lines in bold with their bullets kept on one page, and a
/// section heading never left alone at the bottom of a page.
pub fn html(kind: &str, text: &str, title: &str) -> String {
    text_page(kind, text, title).html()
}

fn text_page(kind: &str, text: &str, title: &str) -> Page {
    let text = normalise(text);
    let body = match kind {
        "resume" | "cv" => resume_html(&text),
        "job" => vec![job_html(&text, title)],
        _ => vec![letter_html(&text)],
    };
    page(body, title)
}

fn resume_fields_page(resume: &TailoredResume, title: &str) -> Page {
    let mut body = vec![Node::Heading1(resume.name.clone())];
    if !resume.title.is_empty() {
        body.push(Node::Paragraph(Paragraph::with_class(
            ParagraphClass::Title,
            vec![resume.title.clone()],
        )));
    }
    let contact: Vec<String> = [tailored_resume::contact(resume), resume.links.join(" · ")]
        .into_iter()
        .filter(|line| php::filled(line))
        .collect();
    if !contact.is_empty() {
        body.push(Node::Paragraph(Paragraph::with_class(ParagraphClass::Contact, contact)));
    }
    for paragraph in &resume.intro {
        body.push(Node::Paragraph(Paragraph::with_class(
            ParagraphClass::Intro,
            vec![paragraph.clone()],
        )));
    }

    body.extend(section(&resume.experience_heading, entries(&resume.experience)));
    body.extend(section(&resume.projects_heading, entries(&resume.projects)));
    body.extend(section(&resume.skills_heading, list_block(&resume.skills)));
    body.extend(section(&resume.languages_heading, list_block(&resume.languages)));
    let mut education = entries(&resume.education);
    if !resume.education_note.is_empty() {
        let mut note = Paragraph::line(resume.education_note.clone());
        note.ends_line = false;
        education.insert(0, Node::Paragraph(note));
    }
    body.extend(section(&resume.education_heading, education));

    page(body, title)
}

/// An entry's heading, meta line, summary, bullets and note.
fn entry(entry: &ResumeEntry) -> Vec<Node> {
    let mut nodes = vec![Node::Heading3(tailored_resume::entry_heading(entry))];
    let meta = tailored_resume::entry_meta(entry);
    if !meta.is_empty() {
        nodes.push(Node::Paragraph(Paragraph::with_class(ParagraphClass::Meta, vec![meta])));
    }
    if !entry.summary.is_empty() {
        nodes.push(Node::Paragraph(Paragraph::line(entry.summary.clone())));
    }
    if !entry.bullets.is_empty() {
        nodes.push(Node::List(entry.bullets.clone()));
    }
    if !entry.note.is_empty() {
        nodes.push(Node::Paragraph(Paragraph::with_class(
            ParagraphClass::Note,
            vec![entry.note.clone()],
        )));
    }
    nodes
}

/// Each entry as a div.entry.
fn entries(list: &[ResumeEntry]) -> Vec<Node> {
    list.iter()
        .map(|item| {
            let mut block = Block::new(BlockClass::Entry, entry(item));
            block.ends_line = false;
            Node::Block(block)
        })
        .collect()
}

/// A list as the one block of a section, or no block when it is empty.
fn list_block(items: &[String]) -> Vec<Node> {
    if items.is_empty() {
        Vec::new()
    } else {
        vec![Node::List(items.to_vec())]
    }
}

/// A section: each block stays on one page; the heading goes with the first block.
fn section(heading: &str, blocks: Vec<Node>) -> Vec<Node> {
    let mut blocks = blocks.into_iter();
    let Some(first) = blocks.next() else {
        return Vec::new();
    };
    let mut nodes = vec![Node::Block(Block::new(
        BlockClass::Keep,
        vec![Node::Heading2(heading.to_string()), first],
    ))];
    nodes.extend(blocks.map(|block| Node::Block(Block::new(BlockClass::Keep, vec![block]))));
    nodes
}

/// The page, in the font that can set everything the body prints. PHP chose the font from the
/// input text (or the résumé's fields) instead, so a job title above the post, which the text
/// doesn't hold, could print "?" for a letter Helvetica lacks.
fn page(body: Vec<Node>, title: &str) -> Page {
    let family = font(body.iter().flat_map(Node::texts));
    Page {
        title: title.to_string(),
        family,
        body,
    }
}

/// Helvetica, as sasin91.xyz's cv.pdf, when every character is in its WinAnsi set (Danish
/// letters are); DejaVu Sans otherwise, so a letter outside it isn't printed as "?".
fn font<'a>(mut texts: impl Iterator<Item = &'a str>) -> Family {
    if texts.all(|text| text.chars().all(|c| fonts::windows_1252(c).is_some())) {
        Family::Helvetica
    } else {
        Family::DejaVuSans
    }
}

/// The application: paragraphs split on blank lines, line breaks kept.
fn letter_html(text: &str) -> Node {
    let paragraphs = BLANK_LINE
        .split(text)
        .map(php::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(|paragraph| {
            Node::Paragraph(Paragraph {
                class: ParagraphClass::Plain,
                lines: paragraph.split('\n').map(str::to_string).collect(),
                breaks: LineBreak::BrNewline,
                ends_line: true,
            })
        })
        .collect();
    let mut letter = Block::new(BlockClass::Letter, paragraphs);
    letter.ends_line = false;
    Node::Block(letter)
}

/// The job post: the title line given, then the text with its "- " lists as bullets and other
/// lines as paragraphs. The text is the post as it was read for the match, so a line that heads
/// a section stays a paragraph.
fn job_html(text: &str, title: &str) -> Node {
    let mut nodes = vec![Node::Heading1(title.to_string())];
    let mut bullets: Vec<String> = Vec::new();
    let end_list = |bullets: &mut Vec<String>, nodes: &mut Vec<Node>| {
        if !bullets.is_empty() {
            nodes.push(Node::List(std::mem::take(bullets)));
        }
    };
    for block in BLANK_LINE.split(php::trim(text)) {
        for line in php::trim(block).split('\n') {
            let line = php::trim(line);
            if let Some(bullet) = bullet_text(line) {
                bullets.push(bullet.to_string());
            } else if !line.is_empty() {
                end_list(&mut bullets, &mut nodes);
                nodes.push(Node::Paragraph(Paragraph::line(line)));
            }
        }
        end_list(&mut bullets, &mut nodes);
    }
    let mut job = Block::new(BlockClass::Job, nodes);
    job.ends_line = false;
    Node::Block(job)
}

/// A bullet line's text, or None for any other line.
fn bullet_text(line: &str) -> Option<&str> {
    BULLET
        .captures(line)
        .and_then(|found| found.get(1))
        .map(|text| text.as_str())
}

/// A piece of the résumé after its header, before headings are kept with what follows them.
enum ResumeBlock {
    Heading(Node),
    Entry(Vec<Node>),
    Text(Vec<Node>),
}

/// The résumé being read line by line: the paragraph and bullets so far, the role (entry) they
/// belong to, and the blocks done.
#[derive(Default)]
struct ResumeReading {
    blocks: Vec<ResumeBlock>,
    entry: Option<Vec<Node>>,
    paragraph: Vec<String>,
    bullets: Vec<String>,
}

impl ResumeReading {
    /// Ends the paragraph and its bullets, into the role when there is one.
    fn end_paragraph(&mut self) {
        let mut nodes = Vec::new();
        if !self.paragraph.is_empty() {
            nodes.push(Node::Paragraph(Paragraph::with_class(
                ParagraphClass::Plain,
                std::mem::take(&mut self.paragraph),
            )));
        }
        if !self.bullets.is_empty() {
            nodes.push(Node::List(std::mem::take(&mut self.bullets)));
        }
        if nodes.is_empty() {
            return;
        }
        match &mut self.entry {
            Some(entry) => entry.extend(nodes),
            None => self.blocks.push(ResumeBlock::Text(nodes)),
        }
    }

    fn end_entry(&mut self) {
        if let Some(entry) = self.entry.take() {
            self.blocks.push(ResumeBlock::Entry(entry));
        }
    }
}

/// The résumé. The first line is the name and the lines up to the first blank line are contact
/// details. After that the text is read by its shape, the way the tailor_resume prompt lays it
/// out, not by what the words say: a heading is a short line on its own after a blank line,
/// with no digits and no comma; a line followed by bullets is a role (or a school) and stays on
/// one page with them; "- " lines are bullets and anything else is a paragraph.
fn resume_html(text: &str) -> Vec<Node> {
    let all: Vec<&str> = php::trim(text).split('\n').collect();
    if all == [""] {
        return Vec::new();
    }
    let mut nodes = vec![Node::Heading1(php::trim(all[0]).to_string())];
    let mut lines = &all[1..];
    let mut contact = Vec::new();
    while let Some((first, rest)) = lines.split_first() {
        if php::trim(first).is_empty() {
            break;
        }
        contact.push(php::trim(first).to_string());
        lines = rest;
    }
    nodes.push(Node::Paragraph(Paragraph::with_class(ParagraphClass::Contact, contact)));

    let mut reading = ResumeReading::default();
    let mut after_heading = false;
    for (i, raw) in lines.iter().enumerate() {
        let line = php::trim(raw);
        // A block starts after a blank line or straight under a heading.
        let after_blank = i == 0 || php::trim(lines[i - 1]).is_empty() || after_heading;
        after_heading = false;
        if line.is_empty() {
            reading.end_paragraph();
        } else if let Some(bullet) = bullet_text(line) {
            if !reading.paragraph.is_empty() {
                reading.end_paragraph();
            }
            reading.bullets.push(bullet.to_string());
        } else if after_blank && is_heading(line, lines.get(i + 1).copied().unwrap_or("")) {
            reading.end_paragraph();
            reading.end_entry();
            let heading = php::trim_end_chars(line, ": ");
            reading
                .blocks
                .push(ResumeBlock::Heading(Node::Heading2(heading.to_string())));
            after_heading = true;
        } else if next_is_bullet(lines, i) || (after_blank && next_is_bullet(lines, i + 1)) {
            // A role: its title line, maybe one line of dates or place, then bullets.
            reading.end_paragraph();
            if after_blank {
                reading.end_entry();
                reading.entry = Some(vec![Node::Heading3(line.to_string())]);
            } else {
                reading
                    .entry
                    .get_or_insert_with(Vec::new)
                    .push(Node::Paragraph(Paragraph::line(line)));
            }
        } else {
            if !reading.bullets.is_empty() {
                reading.end_paragraph();
            }
            if after_blank {
                reading.end_entry();
            }
            reading.paragraph.push(line.to_string());
        }
    }
    reading.end_paragraph();
    reading.end_entry();

    nodes.extend(keep_headings_with_next(reading.blocks));
    nodes
}

/// A heading goes onto the page with the block after it; a role stays on one page.
fn keep_headings_with_next(blocks: Vec<ResumeBlock>) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut blocks = blocks.into_iter().peekable();
    while let Some(block) = blocks.next() {
        match block {
            ResumeBlock::Heading(heading) if blocks.peek().is_some() => {
                let mut kept = vec![heading];
                match blocks.next() {
                    Some(ResumeBlock::Entry(entry)) => kept.push(Node::Block(Block::new(BlockClass::Entry, entry))),
                    Some(ResumeBlock::Heading(next)) => kept.push(next),
                    Some(ResumeBlock::Text(text)) => kept.extend(text),
                    None => {}
                }
                nodes.push(Node::Block(Block::new(BlockClass::Keep, kept)));
            }
            ResumeBlock::Heading(heading) => nodes.push(heading),
            ResumeBlock::Entry(entry) => nodes.push(Node::Block(Block::new(BlockClass::KeepEntry, entry))),
            ResumeBlock::Text(text) => nodes.extend(text),
        }
    }
    nodes
}

/// Whether a line (after a blank line) is a section heading: short, no digits (a role or a
/// school has dates), no comma or separator (a role names its employer), not a sentence, and
/// something follows it.
fn is_heading(line: &str, next: &str) -> bool {
    !php::trim(next).is_empty()
        && line.chars().count() <= MAX_HEADING_LENGTH
        && !NOT_IN_HEADING.is_match(line)
        && !SENTENCE_END.is_match(line)
        && WHITESPACE.split(line).count() <= MAX_HEADING_WORDS
}

fn next_is_bullet(lines: &[&str], i: usize) -> bool {
    lines.get(i + 1).is_some_and(|next| STARTS_AS_BULLET.is_match(next))
}

/// Unix line endings, no trailing spaces, no markdown bold.
fn normalise(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = MARKDOWN_BOLD.replace_all(&text, "$1");
    TRAILING_SPACE.replace_all(&text, "").into_owned()
}

/// A file name for the download: "Application - <job> - <company>.pdf" ("Resume", "CV" or
/// "Job ad" for the other kinds), without characters file systems refuse.
pub fn file_name(kind: &str, job_title: &str, company: &str) -> String {
    let label = FILE_NAMES
        .iter()
        .find(|(name, _)| *name == kind)
        .map_or("Application", |(_, label)| *label);
    let parts: Vec<&str> = [label, php::trim(job_title), php::trim(company)]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    let joined = parts.join(" - ");
    let name = UNSAFE_IN_FILE_NAME.replace_all(&joined, " ");
    let name = WHITESPACE.replace_all(&name, " ");
    format!("{}.pdf", php::first_chars(php::trim(&name), MAX_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_name_the_job_and_company_without_unsafe_characters() {
        assert_eq!(
            file_name("resume", "Udvikler / PHP", "Acme: \"ApS\""),
            "Resume - Udvikler PHP - Acme ApS.pdf"
        );
        assert_eq!(file_name("application", "", ""), "Application.pdf");
        assert_eq!(file_name("job", "Udvikler", "Acme"), "Job ad - Udvikler - Acme.pdf");
    }

    #[test]
    fn the_job_post_page_has_its_title_and_lists() {
        let html = html(
            "job",
            "https://example.dk/job/1\n\nVi søger en <udvikler>.\nDu har:\n- PHP\n- Laravel\n\nSøg nu.",
            "Udvikler · Acme",
        );
        assert!(html.contains(
            "<body>\n<div class=\"job\">\n<h1>Udvikler · Acme</h1>\n<p>https://example.dk/job/1</p>\n<p>Vi søger en &lt;udvikler&gt;.</p>\n<p>Du har:</p>\n<ul>\n<li>PHP</li>\n<li>Laravel</li>\n</ul>\n<p>Søg nu.</p>\n</div>\n</body>"
        ));
    }

    #[test]
    fn a_letter_outside_windows_1252_picks_dejavu_sans() {
        assert_eq!(font(["Kære Søren – “tak” · 5 €"].into_iter()), Family::Helvetica);
        assert_eq!(font(["Kære Søren", "Łódź → Kraków"].into_iter()), Family::DejaVuSans);
    }

    #[test]
    fn the_job_title_counts_when_picking_the_font() {
        let page = text_page("job", "We are hiring in Poland.", "Developer · Łódź Software");
        assert_eq!(page.family, Family::DejaVuSans);
    }
}
