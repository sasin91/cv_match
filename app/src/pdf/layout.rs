//! A page (pdf::page) placed on A4 sheets the way dompdf 3.1 placed this HTML,
//! following the page's style sheet (page::STYLE): each element's font size, line height,
//! weight and margins, the list's bullets and hanging indent, and lines wrapped at spaces.
//!
//! Margins collapse as dompdf collapses them, which is not quite CSS (collapse() says how), and
//! sheets break where dompdf breaks them (Placing::break_before says how): never inside a
//! div.keep that fits on a sheet, never leaving one line of an element behind, and with room
//! for every bottom margin around a line. A box moved to the next sheet keeps its top margin.

use super::fonts::Fonts;
use super::page::{Block, BlockClass, Node, Page, Paragraph, ParagraphClass};

/// Points in a millimetre.
const PT_PER_MM: f32 = 72.0 / 25.4;

/// A4's width and height, in millimetres.
const A4_WIDTH_MM: f32 = 210.0;
const A4_HEIGHT_MM: f32 = 297.0;

/// The page's left and right margins (@page), in millimetres.
const SIDE_MARGIN_MM: f32 = 16.0;

/// The page's top and bottom margins (@page), in millimetres.
const TOP_BOTTOM_MARGIN_MM: f32 = 18.0;

/// How far a list item is indented (li's margin-left), in millimetres.
const LIST_INDENT_MM: f32 = 5.0;

/// How far an item's later lines are indented past its bullet (li's padding-left, with the
/// same negative text-indent for its first line), in millimetres.
const LIST_HANGING_MM: f32 = 3.2;

/// What comes before a list item's text (li:before): a bullet and two no-break spaces.
const BULLET: &str = "\u{2022}\u{a0}\u{a0}";

/// dompdf's font_height_ratio: it sizes a line box as the line-height times the font's height
/// (ascender to descender, in ems) times this, so 14pt lines of Helvetica are 14.245pt apart and
/// of DejaVu Sans 17.926pt.
const DOMPDF_FONT_HEIGHT_RATIO: f32 = 1.1;

/// How far down its line box dompdf puts the baseline, whatever the font size.
const DOMPDF_BASELINE_RATIO: f32 = 0.8;

pub const PAGE_WIDTH: f32 = A4_WIDTH_MM * PT_PER_MM;
pub const PAGE_HEIGHT: f32 = A4_HEIGHT_MM * PT_PER_MM;
const LEFT: f32 = SIDE_MARGIN_MM * PT_PER_MM;
const TOP: f32 = TOP_BOTTOM_MARGIN_MM * PT_PER_MM;
const BOTTOM: f32 = PAGE_HEIGHT - TOP_BOTTOM_MARGIN_MM * PT_PER_MM;
const CONTENT_WIDTH: f32 = PAGE_WIDTH - 2.0 * SIDE_MARGIN_MM * PT_PER_MM;

/// A line of text placed on a sheet: where its baseline starts, measured from the sheet's top
/// left in points, its size and weight, and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub x: f32,
    pub baseline: f32,
    pub size: f32,
    pub bold: bool,
    pub text: String,
}

/// One A4 sheet's lines.
pub type Sheet = Vec<Line>;

/// How an element's text is set: font size and line height in points, and its weight.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TextStyle {
    size: f32,
    line_height: f32,
    bold: bool,
}

/// The body's text (body: 10pt on 14pt).
const BODY: TextStyle = TextStyle {
    size: 10.0,
    line_height: 14.0,
    bold: false,
};

/// An element's top and bottom margins, in points.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Margins {
    top: f32,
    bottom: f32,
}

impl Margins {
    const fn new(top: f32, bottom: f32) -> Self {
        Margins { top, bottom }
    }
}

/// Where in the page an element is: inside the letter or the job post, whose p, h1 and ul have
/// their own style.
#[derive(Debug, Clone, Copy, Default)]
struct Context {
    in_letter: bool,
    in_job: bool,
}

/// An element as a CSS box: its margins, and its text or the boxes inside it.
#[derive(Debug, Clone)]
struct LayoutBox {
    margins: Margins,
    content: BoxContent,
}

#[derive(Debug, Clone)]
enum BoxContent {
    /// An element with text: its style, its lines (split where the HTML has a <br>), and whether
    /// it is a list item.
    Text {
        style: TextStyle,
        lines: Vec<String>,
        list_item: bool,
    },
    /// A div or ul: the boxes inside it, and whether they stay on one page (div.keep).
    Boxes { children: Vec<LayoutBox>, keep: bool },
}

impl LayoutBox {
    fn text(style: TextStyle, margins: Margins, lines: Vec<String>) -> Self {
        LayoutBox {
            margins,
            content: BoxContent::Text {
                style,
                lines,
                list_item: false,
            },
        }
    }
}

fn boxes(nodes: &[Node], context: Context) -> Vec<LayoutBox> {
    nodes.iter().map(|node| layout_box(node, context)).collect()
}

fn layout_box(node: &Node, context: Context) -> LayoutBox {
    match node {
        Node::Heading1(text) => {
            let (style, margins) = heading1_style(context);
            LayoutBox::text(style, margins, vec![text_of(text)])
        }
        Node::Heading2(text) => LayoutBox::text(HEADING2, HEADING2_MARGINS, vec![text_of(text)]),
        Node::Heading3(text) => LayoutBox::text(HEADING3, Margins::default(), vec![text_of(text)]),
        Node::Paragraph(paragraph) => {
            let (style, margins) = paragraph_style(paragraph, context);
            LayoutBox::text(
                style,
                margins,
                paragraph.lines.iter().map(|line| text_of(line)).collect(),
            )
        }
        Node::List(items) => LayoutBox {
            margins: list_margins(context),
            content: BoxContent::Boxes {
                children: items.iter().map(|item| list_item(item)).collect(),
                keep: false,
            },
        },
        Node::Block(block) => block_box(block, context),
    }
}

fn list_item(text: &str) -> LayoutBox {
    LayoutBox {
        margins: Margins::default(),
        content: BoxContent::Text {
            style: BODY,
            lines: vec![text_of(text)],
            list_item: true,
        },
    }
}

fn block_box(block: &Block, context: Context) -> LayoutBox {
    let mut inner = context;
    match block.class {
        BlockClass::Letter => inner.in_letter = true,
        BlockClass::Job => inner.in_job = true,
        _ => {}
    }
    LayoutBox {
        margins: block_margins(block.class),
        content: BoxContent::Boxes {
            children: boxes(&block.children, inner),
            keep: block.class.keeps_together(),
        },
    }
}

/// Collapses the margins of sibling boxes as dompdf does, box by box in the order it reflows them
/// (each box before the boxes inside it): a box's bottom margin takes its next sibling's top
/// margin if larger, its top margin its first child's, and its bottom margin its last child's,
/// each time leaving the other at 0.
fn collapse(siblings: &mut [LayoutBox]) {
    for index in 0..siblings.len() {
        if let Some(next_top) = siblings.get(index + 1).map(|next| next.margins.top) {
            let this = &mut siblings[index].margins;
            this.bottom = this.bottom.max(next_top);
            siblings[index + 1].margins.top = 0.0;
        }
        let this = &mut siblings[index];
        if let BoxContent::Boxes { children, .. } = &mut this.content {
            if let Some(first) = children.first_mut() {
                this.margins.top = this.margins.top.max(first.margins.top);
                first.margins.top = 0.0;
            }
            if let Some(last) = children.last_mut() {
                this.margins.bottom = this.margins.bottom.max(last.margins.bottom);
                last.margins.bottom = 0.0;
            }
            collapse(children);
        }
    }
}

/// What the page-break rules need to know of a box: the box it is in (None for the body), whether
/// a box comes before it in that one, whether it stays on one page (div.keep), its bottom margin
/// once collapsed, and its first piece.
#[derive(Debug, Clone)]
struct BoxInfo {
    parent: Option<usize>,
    has_previous: bool,
    keep: bool,
    bottom: f32,
    first_piece: usize,
}

/// One element with text, before it is wrapped: its style, the space above it (the bottom
/// margins of the boxes before it, and the top margins of the boxes it starts, which stay at
/// the top of a sheet), its lines, whether it is a list item, and its box.
#[derive(Debug, Clone)]
struct Piece {
    style: TextStyle,
    space_above: f32,
    space_top: f32,
    lines: Vec<String>,
    list_item: bool,
    box_id: usize,
}

impl Piece {
    fn space_before(&self) -> f32 {
        self.space_above + self.space_top
    }
}

/// The boxes read into pieces in order, with the margins waiting for the next piece.
#[derive(Default)]
struct Flow {
    pieces: Vec<Piece>,
    boxes: Vec<BoxInfo>,
    space_above: f32,
    space_top: f32,
}

impl Flow {
    fn boxes(&mut self, boxes: &[LayoutBox], parent: Option<usize>) {
        for (index, layout_box) in boxes.iter().enumerate() {
            let box_id = self.boxes.len();
            self.boxes.push(BoxInfo {
                parent,
                has_previous: index > 0,
                keep: matches!(layout_box.content, BoxContent::Boxes { keep: true, .. }),
                bottom: layout_box.margins.bottom,
                first_piece: self.pieces.len(),
            });
            self.space_top += layout_box.margins.top;
            match &layout_box.content {
                BoxContent::Text {
                    style,
                    lines,
                    list_item,
                } => self.text(*style, lines, *list_item, box_id),
                BoxContent::Boxes { children, .. } => self.boxes(children, Some(box_id)),
            }
            self.space_above += layout_box.margins.bottom;
        }
    }

    fn text(&mut self, style: TextStyle, lines: &[String], list_item: bool, box_id: usize) {
        // An empty element has no lines; its margins stay for the next one.
        if lines.iter().all(String::is_empty) && !list_item {
            return;
        }
        self.pieces.push(Piece {
            style,
            space_above: std::mem::take(&mut self.space_above),
            space_top: std::mem::take(&mut self.space_top),
            lines: lines.to_vec(),
            list_item,
            box_id,
        });
    }
}

/// h2: 11pt on 16pt, bold.
const HEADING2: TextStyle = TextStyle {
    size: 11.0,
    line_height: 16.0,
    bold: true,
};
/// h2's margins: 10pt above, 3pt below.
const HEADING2_MARGINS: Margins = Margins::new(10.0, 3.0);

/// h3: 10.5pt on 14pt, bold, no margins.
const HEADING3: TextStyle = TextStyle {
    size: 10.5,
    line_height: 14.0,
    bold: true,
};

/// h1: the name, 20pt on 24pt; in the job post its title, 16pt on 20pt with 8pt below.
fn heading1_style(context: Context) -> (TextStyle, Margins) {
    if context.in_job {
        let style = TextStyle {
            size: 16.0,
            line_height: 20.0,
            bold: true,
        };
        (style, Margins::new(0.0, 8.0))
    } else {
        let style = TextStyle {
            size: 20.0,
            line_height: 24.0,
            bold: true,
        };
        (style, Margins::default())
    }
}

/// A p's style by its class; a plain p has 4pt below, 8pt in the letter.
fn paragraph_style(paragraph: &Paragraph, context: Context) -> (TextStyle, Margins) {
    let sized = |size: f32, line_height: f32| TextStyle {
        size,
        line_height,
        bold: false,
    };
    match paragraph.class {
        ParagraphClass::Title => (sized(11.0, 15.0), Margins::default()),
        ParagraphClass::Contact => (sized(9.0, 13.0), Margins::new(0.0, 8.0)),
        ParagraphClass::Meta => (sized(8.5, 12.0), Margins::default()),
        ParagraphClass::Note => (sized(9.0, 13.0), Margins::new(3.0, 0.0)),
        ParagraphClass::Plain | ParagraphClass::Intro if context.in_letter => (BODY, Margins::new(0.0, 8.0)),
        ParagraphClass::Plain | ParagraphClass::Intro => (BODY, Margins::new(0.0, 4.0)),
    }
}

/// ul has no margins, but 6pt below in the job post.
fn list_margins(context: Context) -> Margins {
    if context.in_job {
        Margins::new(0.0, 6.0)
    } else {
        Margins::default()
    }
}

/// div.entry has 6pt below.
fn block_margins(class: BlockClass) -> Margins {
    match class {
        BlockClass::Entry | BlockClass::KeepEntry => Margins::new(0.0, 6.0),
        _ => Margins::default(),
    }
}

/// A line's text as HTML shows it: runs of white space as one space, none at either end, and
/// no control characters or soft hyphens.
fn text_of(line: &str) -> String {
    let mut text = String::with_capacity(line.len());
    let mut space = false;
    for c in line.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C') {
            space = !text.is_empty();
        } else if !c.is_control() && c != '\u{ad}' {
            if space {
                text.push(' ');
                space = false;
            }
            text.push(c);
        }
    }
    text
}

/// A wrapped line of a piece: where it starts across, and its text.
#[derive(Debug, Clone)]
struct Row {
    x: f32,
    text: String,
}

/// The page's lines on A4 sheets.
pub fn sheets(page: &Page, fonts: &Fonts) -> Vec<Sheet> {
    let mut body = boxes(&page.body, Context::default());
    collapse(&mut body);
    let mut flow = Flow::default();
    flow.boxes(&body, None);
    let rows = flow.pieces.iter().map(|piece| wrap(piece, fonts)).collect();
    Placing::new(&flow, rows, fonts).place()
}

/// The piece's lines wrapped to the page's width.
fn wrap(piece: &Piece, fonts: &Fonts) -> Vec<Row> {
    let font = fonts.get(piece.style.bold);
    let width_of = |text: &str| font.text_width(text) * piece.style.size;
    let (first_x, rest_x) = if piece.list_item {
        let indent = LIST_INDENT_MM * PT_PER_MM;
        (LEFT + indent, LEFT + indent + LIST_HANGING_MM * PT_PER_MM)
    } else {
        (LEFT, LEFT)
    };
    let right = LEFT + CONTENT_WIDTH;
    let mut rows = Vec::new();
    for (index, line) in piece.lines.iter().enumerate() {
        let line = if piece.list_item && index == 0 {
            format!("{BULLET}{line}")
        } else {
            line.clone()
        };
        let mut x = if rows.is_empty() { first_x } else { rest_x };
        for text in wrap_line(&line, &width_of, right - x, right - rest_x) {
            rows.push(Row { x, text });
            x = rest_x;
        }
    }
    rows
}

/// A line split at spaces into rows no wider than `first_width` (the first row) and
/// `rest_width` (the others); a word wider than a row is split between letters. An empty line
/// is one empty row.
fn wrap_line(line: &str, width_of: &dyn Fn(&str) -> f32, first_width: f32, rest_width: f32) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    let mut row = String::new();
    let limit = |rows: &Vec<String>| if rows.is_empty() { first_width } else { rest_width };
    for word in line.split(' ') {
        let candidate = if row.is_empty() {
            word.to_string()
        } else {
            format!("{row} {word}")
        };
        if width_of(&candidate) <= limit(&rows) {
            row = candidate;
            continue;
        }
        if !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        row = split_long_word(word, width_of, &mut rows, &limit);
    }
    rows.push(row);
    rows
}

/// A word wider than a row, its full rows pushed onto `rows`; what is left of it starts the
/// next row.
fn split_long_word(
    word: &str,
    width_of: &dyn Fn(&str) -> f32,
    rows: &mut Vec<String>,
    limit: &dyn Fn(&Vec<String>) -> f32,
) -> String {
    let mut part = String::new();
    for c in word.chars() {
        part.push(c);
        if width_of(&part) > limit(rows) && part.chars().count() > 1 {
            part.pop();
            rows.push(std::mem::take(&mut part));
            part.push(c);
        }
    }
    part
}

/// How far apart a style's lines are: dompdf's line box.
fn line_box(style: TextStyle, fonts: &Fonts) -> f32 {
    style.line_height * fonts.get(style.bold).height() * DOMPDF_FONT_HEIGHT_RATIO
}

/// The most lines of an element dompdf leaves at the bottom of a sheet by themselves (CSS
/// orphans, 2 by default): it breaks before a later line or not inside the element at all.
const ORPHANS: usize = 2;

/// A place in the flow: a piece and one of its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Place {
    piece: usize,
    row: usize,
}

/// Placing the wrapped pieces down the sheets with dompdf's page-break rules.
struct Placing<'a> {
    flow: &'a Flow,
    rows: Vec<Vec<Row>>,
    fonts: &'a Fonts,
    /// The boxes already moved to a new sheet, which dompdf doesn't move again.
    pushed: Vec<bool>,
}

impl<'a> Placing<'a> {
    fn new(flow: &'a Flow, rows: Vec<Vec<Row>>, fonts: &'a Fonts) -> Self {
        Placing {
            flow,
            rows,
            fonts,
            pushed: vec![false; flow.boxes.len()],
        }
    }

    /// The boxes a box is in, innermost first.
    fn ancestors(&self, box_id: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(self.flow.boxes[box_id].parent, |&id| self.flow.boxes[id].parent)
    }

    /// What must fit below a piece's line: the bottom margins of its box and the boxes it is in.
    fn bottom_margins(&self, box_id: usize) -> f32 {
        self.flow.boxes[box_id].bottom + self.ancestors(box_id).map(|id| self.flow.boxes[id].bottom).sum::<f32>()
    }

    /// Whether a box is inside a div.keep.
    fn in_keep(&self, box_id: usize) -> bool {
        self.ancestors(box_id).any(|id| self.flow.boxes[id].keep)
    }

    /// Where the sheet breaks when the row at `overflow` doesn't fit, as dompdf decides: before
    /// that row when enough of its element's lines stay behind and it isn't kept together;
    /// otherwise before the innermost box around it that has a box before it and isn't inside a
    /// div.keep; at the row itself when there is none, or that box was moved before.
    fn break_before(&mut self, overflow: Place) -> Place {
        let box_id = self.flow.pieces[overflow.piece].box_id;
        let line_number = overflow.row + 1;
        if line_number > ORPHANS && !self.in_keep(box_id) {
            return overflow;
        }
        let mut candidate = Some(box_id);
        while let Some(id) = candidate {
            if self.pushed[id] {
                return overflow;
            }
            let info = &self.flow.boxes[id];
            if info.has_previous && !self.in_keep(id) {
                self.pushed[id] = true;
                return Place {
                    piece: info.first_piece,
                    row: 0,
                };
            }
            candidate = info.parent;
        }
        overflow
    }

    /// The pieces placed down the sheets: a new sheet when a row and the bottom margins around
    /// it don't fit, broken where break_before() says. At the top of a sheet only the top
    /// margins of the boxes a piece starts stay above it.
    fn place(mut self) -> Vec<Sheet> {
        let mut sheets: Vec<Vec<(Place, Line)>> = vec![Vec::new()];
        let mut next = Place { piece: 0, row: 0 };
        let mut y = TOP;
        let mut at_top = true;
        'sheet: while next.piece < self.flow.pieces.len() {
            for piece_index in next.piece..self.flow.pieces.len() {
                let piece = &self.flow.pieces[piece_index];
                let first_row = if piece_index == next.piece { next.row } else { 0 };
                if first_row == 0 {
                    y += if at_top { piece.space_top } else { piece.space_before() };
                }
                let style = piece.style;
                let line_box = line_box(style, self.fonts);
                let bottom_margins = self.bottom_margins(piece.box_id);
                for row_index in first_row..self.rows[piece_index].len() {
                    let here = Place {
                        piece: piece_index,
                        row: row_index,
                    };
                    if !at_top && y + line_box + bottom_margins > BOTTOM {
                        let sheet = sheets.last_mut().expect("there is always a sheet");
                        let first_on_sheet = sheet.first().map(|(place, _)| *place);
                        let mut at = self.break_before(here);
                        // A break must leave something on this sheet.
                        if first_on_sheet.is_some_and(|first| at <= first) {
                            at = here;
                        }
                        sheet.retain(|(place, _)| *place < at);
                        sheets.push(Vec::new());
                        next = at;
                        y = TOP;
                        at_top = true;
                        continue 'sheet;
                    }
                    let row = &self.rows[piece_index][row_index];
                    let line = Line {
                        x: row.x,
                        baseline: y + line_box * DOMPDF_BASELINE_RATIO,
                        size: style.size,
                        bold: style.bold,
                        text: row.text.clone(),
                    };
                    sheets.last_mut().expect("there is always a sheet").push((here, line));
                    y += line_box;
                    at_top = false;
                }
            }
            break;
        }
        sheets
            .into_iter()
            .map(|sheet| {
                sheet
                    .into_iter()
                    .map(|(_, line)| line)
                    .filter(|line| !line.text.is_empty())
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::page::{Family, Paragraph};

    fn helvetica() -> Fonts {
        Fonts::of(Family::Helvetica).unwrap()
    }

    fn page(body: Vec<Node>) -> Page {
        Page {
            title: String::new(),
            family: Family::Helvetica,
            body,
        }
    }

    #[test]
    fn html_white_space_collapses() {
        assert_eq!(text_of("  a \t b\u{a0}c\u{ad}\u{1} "), "a b\u{a0}c");
    }

    #[test]
    fn a_long_paragraph_wraps_inside_the_margins() {
        let fonts = helvetica();
        let words = vec!["ord"; 200].join(" ");
        let sheets = sheets(&page(vec![Node::Paragraph(Paragraph::line(words))]), &fonts);
        assert_eq!(sheets.len(), 1);
        assert!(sheets[0].len() > 5);
        for line in &sheets[0] {
            assert!((line.x - LEFT).abs() < f32::EPSILON);
            let width = fonts.regular.text_width(&line.text) * line.size;
            assert!(width <= CONTENT_WIDTH, "{width} > {CONTENT_WIDTH}");
        }
        // One line box apart, as dompdf sets them: 14.245pt for 14pt lines.
        assert!((sheets[0][1].baseline - sheets[0][0].baseline - 14.245).abs() < 0.01);
    }

    #[test]
    fn a_list_item_hangs_under_its_bullet() {
        let fonts = helvetica();
        let item = vec!["punkt"; 60].join(" ");
        let sheets = sheets(&page(vec![Node::List(vec![item])]), &fonts);
        let lines = &sheets[0];
        assert!(lines[0].text.starts_with(BULLET));
        assert!((lines[0].x - (LEFT + LIST_INDENT_MM * PT_PER_MM)).abs() < 0.01);
        assert!((lines[1].x - (LEFT + (LIST_INDENT_MM + LIST_HANGING_MM) * PT_PER_MM)).abs() < 0.01);
    }

    #[test]
    fn a_kept_block_moves_whole_to_the_next_sheet() {
        let fonts = helvetica();
        // 39 lines of 18pt fill all but about 38pt of the first sheet; the heading kept with three
        // lines needs about 70pt.
        let filler: Vec<Node> = (0..39)
            .map(|n| Node::Paragraph(Paragraph::line(format!("Linje {n}"))))
            .collect();
        let kept = Node::Block(Block::new(
            BlockClass::Keep,
            vec![
                Node::Heading2("Erfaring".into()),
                Node::List(vec!["a".into(), "b".into(), "c".into()]),
            ],
        ));
        let mut body = filler;
        body.push(kept);
        let sheets = sheets(&page(body), &fonts);
        assert_eq!(sheets.len(), 2);
        assert_eq!(sheets[1][0].text, "Erfaring");
        // The heading's 10pt top margin comes along, as dompdf keeps it.
        let baseline = TOP + HEADING2_MARGINS.top + line_box(HEADING2, &fonts) * DOMPDF_BASELINE_RATIO;
        assert!((sheets[1][0].baseline - baseline).abs() < 0.01);
    }

    /// The space between the boxes of the first two lines, both 13pt lines (contact and note).
    fn gap(body: Vec<Node>) -> f32 {
        let sheets = sheets(&page(body), &helvetica());
        let line_box = line_box(note_style(), &helvetica());
        let first_bottom = sheets[0][0].baseline + line_box * (1.0 - DOMPDF_BASELINE_RATIO);
        let second_top = sheets[0][1].baseline - line_box * DOMPDF_BASELINE_RATIO;
        second_top - first_bottom
    }

    fn note_style() -> TextStyle {
        TextStyle {
            size: 9.0,
            line_height: 13.0,
            bold: false,
        }
    }

    /// A contact line, with 8pt below it.
    fn contact() -> Node {
        Node::Paragraph(Paragraph::with_class(ParagraphClass::Contact, vec!["a".into()]))
    }

    /// A note, with 3pt above it.
    fn note() -> Node {
        Node::Paragraph(Paragraph::with_class(ParagraphClass::Note, vec!["b".into()]))
    }

    fn kept(node: Node) -> Node {
        Node::Block(Block::new(BlockClass::Keep, vec![node]))
    }

    #[test]
    fn margins_collapse_as_dompdf_collapses_them() {
        // Siblings: 8pt and 3pt make 8pt.
        assert!((gap(vec![contact(), note()]) - 8.0).abs() < 0.01);
        // Into a block: they add up.
        assert!((gap(vec![contact(), kept(note())]) - 11.0).abs() < 0.01);
        // Out of a block: they collapse.
        assert!((gap(vec![kept(contact()), note()]) - 8.0).abs() < 0.01);
    }
}
