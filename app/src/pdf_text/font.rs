//! What the page needs from a PDF font: how codes split, what text they
//! stand for, how far each advances, and its style. The glyphs themselves
//! are drawn by Sappkit in the system's fonts.

use std::collections::HashMap;

use super::document::Document;
use super::encoding::{self, CodeRange, Encoding, ToUnicode};
use super::syntax::{Dict, Object};
use super::truetype;

pub struct Font {
    /// Type 0 fonts take codes of one to four bytes, as their encoding CMap's code space ranges
    /// say (two bytes without ranges, as Identity-H); simple fonts one.
    two_byte: bool,
    code_ranges: Vec<CodeRange>,
    encoding: Encoding,
    to_unicode: Option<ToUnicode>,
    /// For symbolic TrueType fonts, the text the font program itself gives
    /// each code; codes missing from it stand for unknown text.
    program_text: Option<HashMap<u32, String>>,
    /// Advances in thousandths of the font size.
    widths: HashMap<u32, f64>,
    default_width: f64,
    /// Type 3 glyph space to text space, in thousandths.
    scale: f64,
    pub bold: bool,
    pub italic: bool,
    pub family: Family,
}

/// The kind of typeface a PDF font is, so its text can be drawn in a
/// system font of the same kind and metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Family {
    #[default]
    Serif,
    Sans,
    Mono,
}

/// What a font's name and descriptor say about its look.
#[derive(Debug, PartialEq)]
pub struct Style {
    pub family: Family,
    pub bold: bool,
    pub italic: bool,
}

impl Style {
    /// From the base font name, without a subset prefix, the descriptor's
    /// flags and weight. Names follow the PostScript conventions
    /// (`Times-BoldItalic`), the Nimbus ones TeX uses (`NimbusRomNo9L-Medi`,
    /// `-ReguItal`) and Computer Modern's (`CMBX10`, `CMTI10`, `CMTT10`).
    pub fn of(name: &str, flags: i64, weight: f64) -> Style {
        let lower = name.to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
        // Computer Modern and its relatives: the letters after `cm` say
        // the style, as `cmbx` bold extended or `cmtt` typewriter.
        let cm = lower
            .strip_prefix("cm")
            .or_else(|| lower.strip_prefix("lm"))
            .filter(|r| r.starts_with(|c: char| c.is_ascii_alphabetic()));
        let cm_style = |prefixes: &[&str]| cm.is_some_and(|r| prefixes.iter().any(|p| r.starts_with(p)));
        let family = if flags & 1 != 0
            || cm_style(&["tt", "itt", "sltt", "vtt"])
            || has(&[
                "courier",
                "mono",
                "typewriter",
                "consol",
                "menlo",
                "inconsolata",
                "nimbusmon",
            ]) {
            Family::Mono
        } else if cm_style(&["ss"])
            || has(&[
                "helvetica",
                "arial",
                "sans",
                "nimbussan",
                "calibri",
                "verdana",
                "tahoma",
                "segoe",
                "futura",
                "gill",
                "frutiger",
                "myriad",
                "roboto",
                "lato",
                "opensans",
            ])
        {
            Family::Sans
        } else if flags & 2 != 0
            || cm.is_some()
            || has(&[
                "times",
                "roman",
                "nimbusrom",
                "serif",
                "georgia",
                "garamond",
                "palatino",
                "minion",
                "cambria",
                "century",
                "baskerville",
                "utopia",
                "charter",
                "bookman",
                "schoolbook",
                "termes",
                "pagella",
            ])
        {
            Family::Serif
        } else if flags & 32 != 0 {
            // Non-symbolic fonts that say nothing else are mostly body text.
            Family::Serif
        } else {
            Family::Sans
        };
        let bold = weight >= 600.0
            || flags & (1 << 18) != 0
            || cm_style(&["bx", "b1", "bi"])
            || has(&[
                "bold",
                "black",
                "heavy",
                "semibold",
                "demi",
                "-medi",
                "-bd",
                "extrabold",
            ]);
        let italic = (flags & (1 << 6) != 0 && !has(&["symbol", "dingbat"]))
            || cm_style(&["ti", "sl", "mi", "bxti", "bxsl", "itt", "sltt"])
            || has(&["italic", "ital", "oblique", "obli", "-it", "kursiv"]);
        Style { family, bold, italic }
    }
}

impl Font {
    pub fn load(doc: &Document, dict: &Dict) -> Font {
        let subtype = doc.lookup(dict, "Subtype").as_name().unwrap_or("").to_string();
        let base = doc.lookup(dict, "BaseFont").as_name().unwrap_or("").to_string();
        // `ABCDEF+Times-Bold` names a subset of Times-Bold.
        let name = base.split_once('+').map_or(base.as_str(), |(_, n)| n).to_string();
        let two_byte = subtype == "Type0";
        let descendant = match doc.lookup(dict, "DescendantFonts") {
            Object::Array(a) => a.first().map(|d| doc.resolve(d)).and_then(|d| d.as_dict().cloned()),
            _ => None,
        };
        let descriptor_owner = descendant.as_ref().unwrap_or(dict);
        let descriptor = doc
            .lookup(descriptor_owner, "FontDescriptor")
            .as_dict()
            .cloned()
            .unwrap_or_default();
        let flags = doc.lookup(&descriptor, "Flags").as_i64().unwrap_or(0);
        let weight = doc.lookup(&descriptor, "FontWeight").as_f64().unwrap_or(400.0);
        let style = Style::of(&name, flags, weight);
        let (bold, italic, family) = (style.bold, style.italic, style.family);
        let mono = family == Family::Mono;

        let to_unicode = doc
            .stream_data(dict.get("ToUnicode").unwrap_or(&Object::Null))
            .map(|d| ToUnicode::parse(&d));

        let mut widths = HashMap::new();
        let mut default_width;
        if let Some(cid) = &descendant {
            default_width = doc.lookup(cid, "DW").as_f64().unwrap_or(1000.0);
            if let Object::Array(w) = doc.lookup(cid, "W") {
                let w: Vec<Object> = w.iter().map(|o| doc.resolve(o)).collect();
                let mut i = 0;
                while i + 1 < w.len() {
                    let first = w[i].as_i64().unwrap_or(0) as u32;
                    match &w[i + 1] {
                        // `c [w1 w2 ...]`
                        Object::Array(list) => {
                            for (k, v) in list.iter().enumerate() {
                                widths.insert(first + k as u32, doc.resolve(v).as_f64().unwrap_or(default_width));
                            }
                            i += 2;
                        }
                        // `first last w`
                        last => {
                            let last = last.as_i64().unwrap_or(0) as u32;
                            let v = w.get(i + 2).and_then(Object::as_f64).unwrap_or(default_width);
                            for c in first..=last.min(first + 0xffff) {
                                widths.insert(c, v);
                            }
                            i += 3;
                        }
                    }
                }
            }
        } else {
            let first = doc.lookup(dict, "FirstChar").as_i64().unwrap_or(0) as u32;
            if let Object::Array(w) = doc.lookup(dict, "Widths") {
                for (k, v) in w.iter().enumerate() {
                    widths.insert(first + k as u32, doc.resolve(v).as_f64().unwrap_or(0.0));
                }
            }
            default_width = doc.lookup(&descriptor, "MissingWidth").as_f64().unwrap_or(0.0);
            if widths.is_empty() {
                // A standard font without widths: near enough for placing runs.
                default_width = if mono { 600.0 } else { 500.0 };
            }
        }

        let scale = match doc.lookup(dict, "FontMatrix") {
            Object::Array(m) if subtype == "Type3" => {
                m.first().and_then(|v| doc.resolve(v).as_f64()).unwrap_or(0.001) * 1000.0
            }
            _ => 1.0,
        };

        let encoding = if two_byte {
            [None; 256]
        } else {
            simple_encoding(doc, dict, &descriptor, &subtype, &name)
        };
        // A symbolic TrueType font without an encoding addresses its glyphs
        // directly; only the font program knows what they say.
        let program_text = (subtype == "TrueType" && flags & 4 != 0 && !dict.contains_key("Encoding"))
            .then(|| descriptor.get("FontFile2").and_then(|f| doc.stream_data(f)))
            .flatten()
            .map(|program| truetype::code_text(&program));
        // A Type 0 font's embedded encoding CMap says how long its codes are; a named one
        // (Identity-H) takes two bytes.
        let code_ranges = match doc.lookup(dict, "Encoding") {
            Object::Stream(s) if two_byte => doc
                .stream_data(&Object::Stream(s))
                .map(|cmap| encoding::code_ranges(&cmap))
                .unwrap_or_default(),
            _ => vec![],
        };
        Font {
            two_byte,
            code_ranges,
            encoding,
            to_unicode,
            program_text,
            widths,
            default_width,
            scale,
            bold,
            italic,
            family,
        }
    }

    /// Splits a string into codes.
    pub fn codes(&self, bytes: &[u8]) -> Vec<u32> {
        if !self.two_byte {
            return bytes.iter().map(|&b| b as u32).collect();
        }
        if self.code_ranges.is_empty() {
            return bytes.chunks(2).map(encoding::code_of).collect();
        }
        let mut codes = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let length = self.code_length(&bytes[at..]);
            codes.push(encoding::code_of(&bytes[at..at + length]));
            at += length;
        }
        codes
    }

    /// How many bytes the code starting `bytes` takes: the shortest a code space range holds,
    /// else two (as without ranges), or what is left.
    fn code_length(&self, bytes: &[u8]) -> usize {
        (1..=encoding::MAX_CODE_BYTES.min(bytes.len()))
            .find(|&n| {
                let code = encoding::code_of(&bytes[..n]);
                self.code_ranges
                    .iter()
                    .any(|r| r.bytes == n && (r.low..=r.high).contains(&code))
            })
            .unwrap_or(2.min(bytes.len()))
    }

    /// Advance in text space units of one point of font size.
    pub fn advance(&self, code: u32) -> f64 {
        self.widths.get(&code).copied().unwrap_or(self.default_width) * self.scale / 1000.0
    }

    /// The text `code` stands for, or `None` when the PDF does not say.
    pub fn text(&self, code: u32) -> Option<String> {
        if let Some(t) = self.to_unicode.as_ref().and_then(|t| t.map.get(&code)) {
            return Some(t.clone());
        }
        if let Some(program) = &self.program_text {
            return program.get(&code).cloned();
        }
        if self.two_byte {
            return None;
        }
        Some(match self.encoding.get(code as usize).copied().flatten() {
            Some(c) => c.to_string(),
            // Unmapped control codes stand for nothing readable.
            None if code < 32 => String::new(),
            None => char::from_u32(code).map(String::from).unwrap_or_default(),
        })
    }

    /// Whether `code` is a single byte space, which word spacing widens.
    pub fn is_space(&self, code: u32) -> bool {
        !self.two_byte && code == 32
    }
}

fn simple_encoding(doc: &Document, dict: &Dict, descriptor: &Dict, subtype: &str, name: &str) -> Encoding {
    let symbolic = name.contains("Symbol") || name.contains("Dingbats");
    // A Type 1 font's own program may carry the encoding to start from.
    let builtin = || {
        let program = descriptor.get("FontFile").and_then(|f| doc.stream_data(f))?;
        encoding::type1_builtin(&program)
    };
    let fallback = || {
        if subtype == "Type1" {
            builtin().unwrap_or_else(|| {
                if symbolic {
                    encoding::win_ansi()
                } else {
                    encoding::standard()
                }
            })
        } else {
            encoding::win_ansi()
        }
    };
    match doc.lookup(dict, "Encoding") {
        Object::Name(n) => encoding::named(&n).unwrap_or_else(fallback),
        Object::Dict(e) => {
            let mut base = match doc.lookup(&e, "BaseEncoding") {
                Object::Name(n) => encoding::named(&n).unwrap_or_else(fallback),
                _ => fallback(),
            };
            if let Object::Array(diffs) = doc.lookup(&e, "Differences") {
                let mut code = 0usize;
                for d in diffs.iter().map(|d| doc.resolve(d)) {
                    match d {
                        Object::Int(c) => code = c.clamp(0, 255) as usize,
                        Object::Name(n) => {
                            if code < 256 {
                                base[code] = encoding::glyph_name(&n).and_then(|s| s.chars().next());
                            }
                            code += 1;
                        }
                        _ => {}
                    }
                }
            }
            base
        }
        _ => fallback(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Family, Style};

    #[test]
    fn styles_from_names() {
        let style = |name, flags| {
            let s = Style::of(name, flags, 400.0);
            (s.family, s.bold, s.italic)
        };
        assert_eq!(style("NimbusRomNo9L-Medi", 0), (Family::Serif, true, false));
        assert_eq!(style("NimbusRomNo9L-ReguItal", 0), (Family::Serif, false, true));
        assert_eq!(style("NimbusRomNo9L-MediItal", 0), (Family::Serif, true, true));
        assert_eq!(style("Times-Roman", 34), (Family::Serif, false, false));
        assert_eq!(style("Helvetica-BoldOblique", 0), (Family::Sans, true, true));
        assert_eq!(style("Courier", 0), (Family::Mono, false, false));
        assert_eq!(style("CMBX10", 4), (Family::Serif, true, false));
        assert_eq!(style("CMTI10", 4), (Family::Serif, false, true));
        assert_eq!(style("CMTT10", 4), (Family::Mono, false, false));
        assert_eq!(style("CMSS10", 4), (Family::Sans, false, false));
        assert_eq!(style("Calibri-Bold", 4), (Family::Sans, true, false));
        assert_eq!(style("ArialMT", 32), (Family::Sans, false, false));
    }
}
