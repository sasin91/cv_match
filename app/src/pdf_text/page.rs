//! A page's content stream run for its text, from sappkit's pdfreader (page.rs), with what it
//! drew besides text (paths, colors, images, clips) taken out: each glyph the page shows, placed
//! in the page's top left points and gathered into runs of one line and one style. Text the PDF
//! doesn't give as text (no ToUnicode and no known encoding) is left out, not guessed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::document::{Document, PageRef};
use super::font::Font;
use super::syntax::{Dict, Lexer, Object};

/// An open document with the fonts its pages share, loaded once.
pub struct Pdf {
    pub doc: Document,
    fonts: RefCell<HashMap<u32, Rc<Font>>>,
}

/// A page's text runs, in the order the page shows them.
#[derive(Debug, Default)]
pub struct Page {
    pub texts: Vec<TextRun>,
}

/// Text on one baseline in one style.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub x: f32,
    pub baseline: f32,
    pub size: f32,
    /// How wide the PDF's own font sets it.
    pub width: f32,
    pub bold: bool,
    pub italic: bool,
    pub family: super::font::Family,
    /// Where each `char` of `text` starts and ends along x, in page
    /// points: spaces put in for gaps span the gap, and a glyph that reads
    /// as several letters, as a ligature does, shares its width among them.
    pub chars: Vec<(f32, f32)>,
}

type Matrix = [f64; 6];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// How deep forms may draw forms: deeper is a loop or a broken file.
const MAX_FORM_DEPTH: u8 = 12;
/// How many saved graphics states (q) are kept: more is a broken file.
const MAX_SAVED_STATES: usize = 64;

/// `m` then `n`, as PDF's `cm` concatenates.
fn mul(m: &Matrix, n: &Matrix) -> Matrix {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

fn apply(m: &Matrix, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

impl Pdf {
    pub fn open(data: Vec<u8>) -> Result<Pdf, String> {
        Ok(Pdf {
            doc: Document::open(data)?,
            fonts: Default::default(),
        })
    }

    pub fn page_count(&self) -> usize {
        self.doc.pages.len()
    }

    /// Page `i`, run.
    pub fn page(&self, i: usize) -> Page {
        let page = &self.doc.pages[i];
        let mut out = self.interpreter(page);
        let contents = match self.doc.lookup(&page.dict, "Contents") {
            Object::Array(parts) => {
                // The parts join into one stream, split anywhere.
                let mut all = Vec::new();
                for part in &parts {
                    all.extend(self.doc.stream_data(part).unwrap_or_default());
                    all.push(b'\n');
                }
                all
            }
            c => self.doc.stream_data(&c).unwrap_or_default(),
        };
        out.execute(&contents, &page.resources);
        out.flush_accent();
        out.end_run();
        out.page
    }

    /// An interpreter for `page`, from PDF space to the page's top left points.
    fn interpreter(&self, page: &PageRef) -> Interpreter<'_> {
        let [x0, y0, x1, y1] = page.bounds;
        // From PDF space, y up, to the shown page, y down, turned as the
        // page asks.
        let base: Matrix = match page.rotate {
            90 => [0.0, 1.0, 1.0, 0.0, -y0, -x0],
            180 => [-1.0, 0.0, 0.0, 1.0, x1, -y0],
            270 => [0.0, -1.0, -1.0, 0.0, y1, x1],
            _ => [1.0, 0.0, 0.0, -1.0, -x0, y1],
        };
        Interpreter {
            pdf: self,
            page: Page::default(),
            state: State {
                ctm: base,
                ..State::default()
            },
            stack: vec![],
            tm: IDENTITY,
            tlm: IDENTITY,
            run: None,
            accent: None,
            depth: 0,
        }
    }

    fn font(&self, object: &Object) -> Rc<Font> {
        let id = match object {
            Object::Ref(id, _) => Some(*id),
            _ => None,
        };
        if let Some(f) = id.and_then(|id| self.fonts.borrow().get(&id).cloned()) {
            return f;
        }
        let dict = self.doc.resolve(object).as_dict().cloned().unwrap_or_default();
        let font = Rc::new(Font::load(&self.doc, &dict));
        if let Some(id) = id {
            self.fonts.borrow_mut().insert(id, Rc::clone(&font));
        }
        font
    }
}

#[derive(Clone)]
struct State {
    ctm: Matrix,
    font: Option<Rc<Font>>,
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
    render_mode: i64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ctm: IDENTITY,
            font: None,
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: 0,
        }
    }
}

struct Interpreter<'a> {
    pdf: &'a Pdf,
    page: Page,
    state: State,
    stack: Vec<State>,
    tm: Matrix,
    tlm: Matrix,
    /// The run being built, with where its last glyph ended.
    run: Option<(TextRun, f32)>,
    /// A spacing accent waiting to see whether the next glyph is the
    /// letter it sits on.
    accent: Option<Accent>,
    depth: u8,
}

/// An accent TeX set as a glyph of its own over a letter, as `\"\i`.
struct Accent {
    mark: char,
    text: String,
    x: f32,
    baseline: f32,
    end: f32,
    size: f32,
    font: Rc<Font>,
}

/// The combining mark a spacing accent stands for.
fn combining(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(match c {
        '¨' => '\u{308}',
        '´' => '\u{301}',
        '`' => '\u{300}',
        'ˆ' => '\u{302}',
        '˜' => '\u{303}',
        '¯' => '\u{304}',
        '˘' => '\u{306}',
        '˙' => '\u{307}',
        '˚' => '\u{30a}',
        '¸' => '\u{327}',
        'ˇ' => '\u{30c}',
        '˝' => '\u{30b}',
        '˛' => '\u{328}',
        _ => return None,
    })
}

/// Whether an accent from `x` to `end` sits over a letter from `start` to
/// `stop`: its middle is over the letter.
fn over(x: f32, end: f32, start: f32, stop: f32, size: f32) -> bool {
    let middle = (x + end) / 2.0;
    middle >= start - 0.1 * size && middle <= stop + 0.1 * size
}

/// A letter with `mark` on it: the dotless i and j TeX puts accents on
/// become plain ones; shaping draws the pair as the accented letter.
fn accented(letter: &str, mark: char) -> String {
    let base: String = letter
        .chars()
        .map(|c| match c {
            'ı' => 'i',
            'ȷ' => 'j',
            c => c,
        })
        .collect();
    format!("{base}{mark}")
}

impl Interpreter<'_> {
    fn execute(&mut self, content: &[u8], resources: &Dict) {
        let doc = &self.pdf.doc;
        let mut lex = Lexer::new(content, 0);
        let mut operands: Vec<Object> = Vec::new();
        loop {
            // Operators are read in place: a chart can have tens of
            // thousands, and allocating each one's name showed in profiles.
            let op = match lex.operator() {
                Some(op) => op,
                None => match lex.object(false) {
                    Some(Object::Keyword(_)) => {
                        // A stray delimiter: skip it as an operator would end.
                        operands.clear();
                        continue;
                    }
                    Some(object) => {
                        operands.push(object);
                        continue;
                    }
                    None => break,
                },
            };
            if op == "BI" {
                skip_inline_image(&mut lex);
                operands.clear();
                continue;
            }
            let n = |i: usize| operands.get(i).and_then(Object::as_f64).unwrap_or(0.0);
            match op {
                "q" => {
                    if self.stack.len() < MAX_SAVED_STATES {
                        self.stack.push(self.state.clone());
                    }
                }
                "Q" => {
                    if let Some(s) = self.stack.pop() {
                        self.state = s;
                    }
                }
                "cm" => {
                    let m = [n(0), n(1), n(2), n(3), n(4), n(5)];
                    self.state.ctm = mul(&m, &self.state.ctm);
                }
                // Text.
                "BT" => {
                    self.tm = IDENTITY;
                    self.tlm = IDENTITY;
                }
                "Tf" => {
                    let fonts = doc.lookup(resources, "Font");
                    let font = operands
                        .first()
                        .and_then(Object::as_name)
                        .and_then(|name| fonts.as_dict()?.get(name).cloned());
                    self.state.font = font.map(|f| self.pdf.font(&f));
                    self.state.font_size = n(1);
                }
                "Tc" => self.state.char_spacing = n(0),
                "Tw" => self.state.word_spacing = n(0),
                "Tz" => self.state.h_scale = n(0) / 100.0,
                "TL" => self.state.leading = n(0),
                "Ts" => self.state.rise = n(0),
                "Tr" => self.state.render_mode = n(0) as i64,
                "Td" => self.next_line(n(0), n(1)),
                "TD" => {
                    self.state.leading = -n(1);
                    self.next_line(n(0), n(1));
                }
                "Tm" => {
                    self.tm = [n(0), n(1), n(2), n(3), n(4), n(5)];
                    self.tlm = self.tm;
                }
                "T*" => self.next_line(0.0, -self.state.leading),
                "Tj" => {
                    if let Some(Object::String(s)) = operands.first() {
                        self.show(s);
                    }
                }
                "'" => {
                    self.next_line(0.0, -self.state.leading);
                    if let Some(Object::String(s)) = operands.first() {
                        self.show(s);
                    }
                }
                "\"" => {
                    self.state.word_spacing = n(0);
                    self.state.char_spacing = n(1);
                    self.next_line(0.0, -self.state.leading);
                    if let Some(Object::String(s)) = operands.get(2) {
                        self.show(s);
                    }
                }
                "TJ" => {
                    if let Some(Object::Array(items)) = operands.first() {
                        for item in items {
                            match item {
                                Object::String(s) => self.show(s),
                                other => {
                                    let adjust = other.as_f64().unwrap_or(0.0);
                                    self.shift(-adjust / 1000.0 * self.state.font_size * self.state.h_scale);
                                }
                            }
                        }
                    }
                }
                "Do" => {
                    if let Some(name) = operands.first().and_then(Object::as_name) {
                        let xobjects = doc.lookup(resources, "XObject");
                        if let Some(x) = xobjects.as_dict().and_then(|d| d.get(name)).cloned() {
                            self.form(&x, resources);
                        }
                    }
                }
                _ => {}
            }
            operands.clear();
        }
    }

    fn next_line(&mut self, tx: f64, ty: f64) {
        self.tlm = mul(&[1.0, 0.0, 0.0, 1.0, tx, ty], &self.tlm);
        self.tm = self.tlm;
    }

    /// Moves along the line by `tx` text space units.
    fn shift(&mut self, tx: f64) {
        self.tm = mul(&[1.0, 0.0, 0.0, 1.0, tx, 0.0], &self.tm);
    }

    fn show(&mut self, bytes: &[u8]) {
        let Some(font) = self.state.font.clone() else {
            return;
        };
        let s = self.state.clone();
        let size = s.font_size;
        // Invisible text, as OCR layers over scans use, and clipping only
        // modes draw nothing.
        let visible = !matches!(s.render_mode, 3 | 7);
        for code in font.codes(bytes) {
            let advance =
                (font.advance(code) * size + s.char_spacing + if font.is_space(code) { s.word_spacing } else { 0.0 })
                    * s.h_scale;
            let m = mul(&self.tm, &self.state.ctm);
            let (x, y) = apply(&m, 0.0, self.state.rise);
            let device_size = (size * (m[2] * m[2] + m[3] * m[3]).sqrt()) as f32;
            // Only upright text: turned text (a margin note up the side) is left out.
            let upright = m[1].abs() < 1e-3 * m[0].abs().max(1e-9) && m[0] > 0.0 && m[3] < 0.0;
            let Some(text) = font.text(code) else {
                // Text the PDF does not give is left out, not guessed.
                if visible {
                    self.end_run();
                }
                self.shift(advance);
                continue;
            };
            if visible && upright && device_size > 0.5 {
                let end = apply(&mul(&[1.0, 0.0, 0.0, 1.0, advance, 0.0], &m), 0.0, 0.0).0 as f32;
                let (x, y) = (x as f32, y as f32);
                match combining(&text) {
                    // An accent over the letter before it joins that letter;
                    // otherwise it waits for the letter after it.
                    Some(mark) => {
                        if !self.accent_last(mark, x, y, end, device_size) {
                            self.flush_accent();
                            let font = Rc::clone(&font);
                            self.accent = Some(Accent {
                                mark,
                                text,
                                x,
                                baseline: y,
                                end,
                                size: device_size,
                                font,
                            });
                        }
                    }
                    None => {
                        let text = match self.accent.take() {
                            Some(a)
                                if (a.baseline - y).abs() < device_size && over(a.x, a.end, x, end, device_size) =>
                            {
                                accented(&text, a.mark)
                            }
                            Some(a) => {
                                self.glyph(&a.text, a.x, a.baseline, a.end, a.size, &a.font);
                                text
                            }
                            None => text,
                        };
                        self.glyph(&text, x, y, end, device_size, &font);
                    }
                }
            }
            self.shift(advance);
        }
    }

    /// Puts `mark` on the last letter of the current run when the accent
    /// sits over it. Returns whether it did.
    fn accent_last(&mut self, mark: char, x: f32, baseline: f32, end: f32, size: f32) -> bool {
        let Some((run, _)) = &mut self.run else { return false };
        let Some(&(start, stop)) = run.chars.last() else {
            return false;
        };
        // Only when well inside it: an accent at the edge of the letter
        // before belongs to the letter after, where TeX sets it first.
        let middle = (x + end) / 2.0;
        if (run.baseline - baseline).abs() >= size || middle < start || middle > stop - 0.1 * size {
            return false;
        }
        let last = run.text.pop().unwrap_or(' ');
        run.text.push_str(&accented(&last.to_string(), mark));
        // The mark is a character of its own, over the letter's span.
        run.chars.push((start, stop));
        true
    }

    /// Draws a waiting accent as it is, over no letter.
    fn flush_accent(&mut self) {
        if let Some(a) = self.accent.take() {
            self.glyph(&a.text, a.x, a.baseline, a.end, a.size, &a.font);
        }
    }

    /// Adds a glyph's text to the current run, or starts a new run.
    fn glyph(&mut self, text: &str, x: f32, baseline: f32, end: f32, size: f32, font: &Font) {
        let text = expand_ligatures(text);
        if let Some((run, run_end)) = &mut self.run {
            let same_style = (run.size - size).abs() < size * 0.05
                && run.bold == font.bold
                && run.italic == font.italic
                && run.family == font.family;
            let same_line = (run.baseline - baseline).abs() < size * 0.2;
            let gap = x - *run_end;
            if same_style && same_line && gap > -size * 0.6 && gap < size * 2.5 {
                // Writers leave spaces out and move instead, as TeX does.
                if gap > size * 0.15 && !run.text.ends_with(' ') && !text.starts_with(' ') {
                    run.text.push(' ');
                    run.chars.push((*run_end, x));
                }
                run.text.push_str(&text);
                spans(&mut run.chars, &text, x, end);
                run.width = end - run.x;
                *run_end = end;
                return;
            }
        }
        self.end_run();
        if text.trim().is_empty() {
            return;
        }
        let mut chars = Vec::with_capacity(text.len());
        spans(&mut chars, &text, x, end);
        let run = TextRun {
            chars,
            text,
            x,
            baseline,
            size,
            width: end - x,
            bold: font.bold,
            italic: font.italic,
            family: font.family,
        };
        self.run = Some((run, end));
    }

    fn end_run(&mut self) {
        if let Some((mut run, _)) = self.run.take() {
            let trimmed = run.text.trim_end().len();
            run.text.truncate(trimmed);
            run.chars.truncate(run.text.chars().count());
            if !run.text.is_empty() {
                self.page.texts.push(run);
            }
        }
    }

    /// Runs a form XObject's content, as `Do` does; images and other XObjects have no text.
    fn form(&mut self, object: &Object, resources: &Dict) {
        let doc = &self.pdf.doc;
        let Object::Stream(stream) = doc.resolve(object) else {
            return;
        };
        if doc.lookup(&stream.dict, "Subtype").as_name() != Some("Form") || self.depth >= MAX_FORM_DEPTH {
            return;
        }
        let Some(content) = doc.decode(&stream).map(|d| d.data) else {
            return;
        };
        let own = match doc.lookup(&stream.dict, "Resources") {
            Object::Dict(r) => r,
            _ => resources.clone(),
        };
        let saved = (self.state.clone(), self.stack.len());
        if let Object::Array(m) = doc.lookup(&stream.dict, "Matrix") {
            let v: Vec<f64> = m.iter().filter_map(|x| doc.resolve(x).as_f64()).collect();
            if v.len() == 6 {
                self.state.ctm = mul(&[v[0], v[1], v[2], v[3], v[4], v[5]], &self.state.ctm);
            }
        }
        self.depth += 1;
        self.execute(&content, &own);
        self.depth -= 1;
        self.stack.truncate(saved.1);
        self.state = saved.0;
    }
}

/// Skips an inline image (BI ... ID data EI): its data up to `EI` on its own.
fn skip_inline_image(lex: &mut Lexer) {
    loop {
        match lex.object(false) {
            Some(o) if o.is_keyword("ID") => break,
            Some(_) => continue,
            None => return,
        }
    }
    let data = lex.data;
    let mut end = lex.pos + 1;
    while end + 2 <= data.len() {
        if &data[end..end + 2] == b"EI"
            && data
                .get(end.wrapping_sub(1))
                .is_some_and(|&b| super::syntax::is_white(b))
            && data.get(end + 2).is_none_or(|&b| super::syntax::is_white(b))
        {
            break;
        }
        end += 1;
    }
    lex.pos = (end + 2).min(data.len());
}

/// The spans of a glyph's chars: `x` to `end` shared equally.
fn spans(out: &mut Vec<(f32, f32)>, text: &str, x: f32, end: f32) {
    let n = text.chars().count().max(1) as f32;
    let step = (end - x) / n;
    out.extend((0..text.chars().count()).map(|i| {
        let i = i as f32;
        (x + step * i, x + step * (i + 1.0))
    }));
}

/// Ligature glyphs as their letters, so the text reads and wraps as text.
fn expand_ligatures(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'ﬀ' => out.push_str("ff"),
            'ﬁ' => out.push_str("fi"),
            'ﬂ' => out.push_str("fl"),
            'ﬃ' => out.push_str("ffi"),
            'ﬄ' => out.push_str("ffl"),
            c => out.push(c),
        }
    }
    out
}
