//! The fonts a PDF page is set in: Helvetica and Helvetica-Bold, the PDF base fonts every
//! reader has, so nothing is embedded; or DejaVu Sans and DejaVu Sans Bold, built into the app
//! and embedded (only the letters used) for text Helvetica can't print.

use ttf_parser::{Face, GlyphId};

use super::page::Family;

/// DejaVu Sans 2.37 (assets/fonts, Bitstream Vera licence).
const DEJAVU_SANS: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");
const DEJAVU_SANS_BOLD: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans-Bold.ttf");

/// The units Helvetica's metrics, and dompdf's metrics of every font, are given in: thousandths
/// of an em.
const METRIC_UNITS_PER_EM: f32 = 1000.0;

/// How far Helvetica rises above the baseline and falls below it together, in thousandths of an
/// em (its AFM's Ascender 718 and Descender -207).
const HELVETICA_HEIGHT: f32 = 718.0 + 207.0;

/// A font a page is set in.
pub enum Font {
    /// Helvetica (bold or not), encoded as WinAnsi.
    Helvetica { bold: bool },
    /// A TrueType font embedded in the PDF.
    TrueType(Box<TrueType>),
}

/// A TrueType font built into the app: its name, its file and the file read.
pub struct TrueType {
    pub name: &'static str,
    pub data: &'static [u8],
    pub face: Face<'static>,
}

/// A family's regular and bold fonts.
pub struct Fonts {
    pub regular: Font,
    pub bold: Font,
}

impl Fonts {
    /// The fonts of a family; Err when a built-in font can't be read.
    pub fn of(family: Family) -> Result<Fonts, String> {
        Ok(match family {
            Family::Helvetica => Fonts {
                regular: Font::Helvetica { bold: false },
                bold: Font::Helvetica { bold: true },
            },
            Family::DejaVuSans => Fonts {
                regular: Font::TrueType(Box::new(TrueType::read("DejaVuSans", DEJAVU_SANS)?)),
                bold: Font::TrueType(Box::new(TrueType::read("DejaVuSans-Bold", DEJAVU_SANS_BOLD)?)),
            },
        })
    }

    pub fn get(&self, bold: bool) -> &Font {
        if bold { &self.bold } else { &self.regular }
    }
}

impl TrueType {
    fn read(name: &'static str, data: &'static [u8]) -> Result<TrueType, String> {
        let face = Face::parse(data, 0).map_err(|e| format!("The built-in font {name} couldn't be read: {e}"))?;
        Ok(TrueType { name, data, face })
    }

    /// The letter's glyph, or the font's "missing letter" box (glyph 0).
    pub fn glyph(&self, c: char) -> GlyphId {
        self.face.glyph_index(c).unwrap_or(GlyphId(0))
    }

    /// A glyph's advance width in ems, to the thousandth of an em as dompdf's font metrics
    /// (.ufm) have it, so lines wrap where they wrapped in the PHP app.
    pub fn advance(&self, glyph: GlyphId) -> f32 {
        thousandths(
            self.face.glyph_hor_advance(glyph).unwrap_or(0).into(),
            self.units_per_em(),
        )
    }

    pub fn units_per_em(&self) -> f32 {
        f32::from(self.face.units_per_em())
    }
}

impl Font {
    /// How wide a letter is, in ems.
    pub fn width(&self, c: char) -> f32 {
        match self {
            Font::Helvetica { bold } => {
                let widths = if *bold { &HELVETICA_BOLD } else { &HELVETICA };
                let code = windows_1252(c).unwrap_or(b'?');
                f32::from(widths[usize::from(code)]) / METRIC_UNITS_PER_EM
            }
            Font::TrueType(font) => font.advance(font.glyph(c)),
        }
    }

    /// How far the font rises above the baseline and falls below it together, in ems.
    pub fn height(&self) -> f32 {
        match self {
            Font::Helvetica { .. } => HELVETICA_HEIGHT / METRIC_UNITS_PER_EM,
            Font::TrueType(font) => {
                let units = font.units_per_em();
                thousandths(font.face.ascender().into(), units) - thousandths(font.face.descender().into(), units)
            }
        }
    }

    /// How wide a text is, in ems.
    pub fn text_width(&self, text: &str) -> f32 {
        text.chars().map(|c| self.width(c)).sum()
    }
}

/// A length in font units in ems, rounded to the thousandth of an em.
fn thousandths(units: f32, units_per_em: f32) -> f32 {
    (units * METRIC_UNITS_PER_EM / units_per_em).round() / METRIC_UNITS_PER_EM
}

/// The letters Windows-1252 has at 0x80-0x9F, where Latin-1 has control codes; 0x81, 0x8D,
/// 0x8F, 0x90 and 0x9D are unused.
const WINDOWS_1252_HIGH: [(char, u8); 27] = [
    ('€', 0x80),
    ('‚', 0x82),
    ('ƒ', 0x83),
    ('„', 0x84),
    ('…', 0x85),
    ('†', 0x86),
    ('‡', 0x87),
    ('ˆ', 0x88),
    ('‰', 0x89),
    ('Š', 0x8A),
    ('‹', 0x8B),
    ('Œ', 0x8C),
    ('Ž', 0x8E),
    ('‘', 0x91),
    ('’', 0x92),
    ('“', 0x93),
    ('”', 0x94),
    ('•', 0x95),
    ('–', 0x96),
    ('—', 0x97),
    ('˜', 0x98),
    ('™', 0x99),
    ('š', 0x9A),
    ('›', 0x9B),
    ('œ', 0x9C),
    ('ž', 0x9E),
    ('Ÿ', 0x9F),
];

/// The letter's byte in Windows-1252 (the PDF's WinAnsi encoding), or None when it has none,
/// as PHP's iconv('UTF-8', 'Windows-1252') decides.
pub fn windows_1252(c: char) -> Option<u8> {
    let code = u32::from(c);
    let latin_1 = code < 0x80 || (0xA0..=0xFF).contains(&code);
    if latin_1 {
        return u8::try_from(code).ok();
    }
    WINDOWS_1252_HIGH
        .iter()
        .find(|(letter, _)| *letter == c)
        .map(|(_, byte)| *byte)
}

/// Helvetica's advance widths in thousandths of an em, by WinAnsi code (0-255), from its AFM.
#[rustfmt::skip]
const HELVETICA: [u16; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, 350,
    556, 350, 222, 556, 333, 1000, 556, 556, 333, 1000, 667, 333, 1000, 350, 611, 350,
    350, 222, 222, 333, 333, 350, 556, 1000, 333, 1000, 500, 333, 944, 350, 500, 667,
    278, 333, 556, 556, 556, 556, 260, 556, 333, 737, 370, 556, 584, 333, 737, 333,
    400, 584, 333, 333, 333, 556, 537, 278, 333, 333, 365, 556, 834, 834, 834, 611,
    667, 667, 667, 667, 667, 667, 1000, 722, 667, 667, 667, 667, 278, 278, 278, 278,
    722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722, 722, 667, 667, 611,
    556, 556, 556, 556, 556, 556, 889, 500, 556, 556, 556, 556, 278, 278, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 584, 611, 556, 556, 556, 556, 500, 556, 500,
];

/// Helvetica-Bold's advance widths in thousandths of an em, by WinAnsi code (0-255), from its AFM.
#[rustfmt::skip]
const HELVETICA_BOLD: [u16; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611,
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556,
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611,
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584, 350,
    556, 350, 278, 556, 500, 1000, 556, 556, 333, 1000, 667, 333, 1000, 350, 611, 350,
    350, 278, 278, 500, 500, 350, 556, 1000, 333, 1000, 556, 333, 944, 350, 500, 667,
    278, 333, 556, 556, 556, 556, 280, 556, 333, 737, 370, 556, 584, 333, 737, 333,
    400, 584, 333, 333, 333, 611, 556, 278, 333, 333, 365, 556, 834, 834, 834, 611,
    722, 722, 722, 722, 722, 722, 1000, 722, 667, 667, 667, 667, 278, 278, 278, 278,
    722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722, 722, 667, 667, 611,
    556, 556, 556, 556, 556, 556, 889, 556, 556, 556, 556, 556, 278, 278, 278, 278,
    611, 611, 611, 611, 611, 611, 611, 584, 611, 611, 611, 611, 611, 556, 611, 556,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_1252_has_the_danish_letters_and_typographic_marks() {
        assert_eq!(windows_1252('ø'), Some(0xF8));
        assert_eq!(windows_1252('–'), Some(0x96));
        assert_eq!(windows_1252('€'), Some(0x80));
        assert_eq!(windows_1252('\u{81}'), None);
        assert_eq!(windows_1252('→'), None);
    }

    #[test]
    fn helvetica_is_measured_by_its_afm() {
        let font = Font::Helvetica { bold: false };
        assert_eq!(font.text_width("Hi"), (722.0 + 222.0) / 1000.0);
    }
}
