//! The page Pdf_writer lays a document out on: the few HTML elements it prints
//! (h1, h2, h3, p, ul and div), as a tree that prints the same HTML (html(), which
//! the parity tests compare) and that layout::pages() places on A4.

/// The two font families Pdf_writer::font() picks from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// The PDF base font, for text that fits Windows-1252 (Danish letters do).
    Helvetica,
    /// DejaVu Sans, built in, for text with a letter Helvetica lacks.
    DejaVuSans,
}

impl Family {
    /// The page's CSS font-family.
    pub fn css(self) -> &'static str {
        match self {
            Family::Helvetica => "Helvetica, sans-serif",
            Family::DejaVuSans => "\"DejaVu Sans\", sans-serif",
        }
    }
}

/// A document laid out for print: its title (the PDF's metadata), its font and its body.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub title: String,
    pub family: Family,
    pub body: Vec<Node>,
}

/// One element of the page. The texts are plain; html() escapes them.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Heading1(String),
    Heading2(String),
    Heading3(String),
    Paragraph(Paragraph),
    List(Vec<String>),
    Block(Block),
}

/// A p: its lines, which line breaks split, and its class.
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    pub class: ParagraphClass,
    pub lines: Vec<String>,
    pub breaks: LineBreak,
    /// A newline follows it in the HTML.
    pub ends_line: bool,
}

/// How a paragraph's lines are joined in the HTML: "<br>", or nl2br's "<br>\n".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineBreak {
    Br,
    BrNewline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphClass {
    Plain,
    Title,
    Contact,
    Intro,
    Meta,
    Note,
}

impl ParagraphClass {
    fn attribute(self) -> &'static str {
        match self {
            ParagraphClass::Plain => "",
            ParagraphClass::Title => " class=\"title\"",
            ParagraphClass::Contact => " class=\"contact\"",
            ParagraphClass::Intro => " class=\"intro\"",
            ParagraphClass::Meta => " class=\"meta\"",
            ParagraphClass::Note => " class=\"note\"",
        }
    }
}

/// A div grouping nodes: kept on one page (keep), an entry's spacing (entry), or the letter's
/// and the job post's own styles.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub class: BlockClass,
    pub children: Vec<Node>,
    /// A newline follows it in the HTML.
    pub ends_line: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockClass {
    Keep,
    Entry,
    KeepEntry,
    Letter,
    Job,
}

impl BlockClass {
    fn name(self) -> &'static str {
        match self {
            BlockClass::Keep => "keep",
            BlockClass::Entry => "entry",
            BlockClass::KeepEntry => "keep entry",
            BlockClass::Letter => "letter",
            BlockClass::Job => "job",
        }
    }

    /// Whether the block stays on one page (page-break-inside: avoid).
    pub fn keeps_together(self) -> bool {
        matches!(self, BlockClass::Keep | BlockClass::KeepEntry)
    }
}

impl Paragraph {
    /// A plain p of one line.
    pub fn line(text: impl Into<String>) -> Self {
        Paragraph {
            class: ParagraphClass::Plain,
            lines: vec![text.into()],
            breaks: LineBreak::Br,
            ends_line: true,
        }
    }

    pub fn with_class(class: ParagraphClass, lines: Vec<String>) -> Self {
        Paragraph {
            class,
            lines,
            breaks: LineBreak::Br,
            ends_line: true,
        }
    }
}

impl Block {
    pub fn new(class: BlockClass, children: Vec<Node>) -> Self {
        Block {
            class,
            children,
            ends_line: true,
        }
    }
}

/// The page's style sheet; layout follows it.
const STYLE: &str = r#"    @page { margin: 18mm 16mm; }
    body { font-family: FONT; font-size: 10pt; line-height: 14pt; color: #000; }
    p { margin: 0 0 4pt 0; }
    h1 { font-size: 20pt; line-height: 24pt; font-weight: bold; margin: 0; }
    .title { font-size: 11pt; line-height: 15pt; margin: 0; }
    .contact { font-size: 9pt; line-height: 13pt; margin: 0 0 8pt 0; }
    .meta { font-size: 8.5pt; line-height: 12pt; margin: 0; }
    .note { font-size: 9pt; line-height: 13pt; margin: 3pt 0 0 0; }
    h2 { font-size: 11pt; line-height: 16pt; font-weight: bold; margin: 10pt 0 3pt 0; }
    h3 { font-size: 10.5pt; line-height: 14pt; font-weight: bold; margin: 0; }
    ul { margin: 0; padding: 0; list-style: none; }
    li { margin: 0 0 0 5mm; padding-left: 3.2mm; text-indent: -3.2mm; }
    li:before { content: "\2022\00a0\00a0"; }
    .keep { page-break-inside: avoid; }
    .entry { margin: 0 0 6pt 0; }
    .letter p { margin: 0 0 8pt 0; }
    .job h1 { font-size: 16pt; line-height: 20pt; margin: 0 0 8pt 0; }
    .job ul { margin: 0 0 6pt 0; }"#;

impl Page {
    /// The page as HTML.
    pub fn html(&self) -> String {
        format!(
            "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>\n{}\n</style>\n</head>\n<body>\n{}\n</body>\n</html>",
            e(&self.title),
            STYLE.replace("FONT", self.family.css()),
            nodes_html(&self.body)
        )
    }
}

impl Node {
    /// Every text the node prints, in order: what the page's font has to be able to set.
    pub fn texts(&self) -> Vec<&str> {
        match self {
            Node::Heading1(text) | Node::Heading2(text) | Node::Heading3(text) => vec![text],
            Node::Paragraph(paragraph) => paragraph.lines.iter().map(String::as_str).collect(),
            Node::List(items) => items.iter().map(String::as_str).collect(),
            Node::Block(block) => block.children.iter().flat_map(Node::texts).collect(),
        }
    }
}

/// The nodes' HTML, one after the other.
pub fn nodes_html(nodes: &[Node]) -> String {
    nodes.iter().map(node_html).collect()
}

fn node_html(node: &Node) -> String {
    match node {
        Node::Heading1(text) => format!("<h1>{}</h1>\n", e(text)),
        Node::Heading2(text) => format!("<h2>{}</h2>\n", e(text)),
        Node::Heading3(text) => format!("<h3>{}</h3>\n", e(text)),
        Node::Paragraph(paragraph) => paragraph_html(paragraph),
        Node::List(items) => list_html(items),
        Node::Block(block) => format!(
            "<div class=\"{}\">\n{}</div>{}",
            block.class.name(),
            nodes_html(&block.children),
            line_end(block.ends_line)
        ),
    }
}

fn paragraph_html(paragraph: &Paragraph) -> String {
    let separator = match paragraph.breaks {
        LineBreak::Br => "<br>",
        LineBreak::BrNewline => "<br>\n",
    };
    let lines: Vec<String> = paragraph.lines.iter().map(|line| e(line)).collect();
    format!(
        "<p{}>{}</p>{}",
        paragraph.class.attribute(),
        lines.join(separator),
        line_end(paragraph.ends_line)
    )
}

fn list_html(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|item| e(item)).collect();
    format!("<ul>\n<li>{}</li>\n</ul>\n", items.join("</li>\n<li>"))
}

fn line_end(ends_line: bool) -> &'static str {
    if ends_line { "\n" } else { "" }
}

/// & < > " and ' as HTML entities.
pub fn e(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            _ => out.push(c),
        }
    }
    out
}
