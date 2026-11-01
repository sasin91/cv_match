//! From a font's character codes to Unicode: the standard encodings,
//! glyph names, and ToUnicode maps.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::syntax::{Lexer, Object};

pub type Encoding = [Option<char>; 256];

/// WinAnsiEncoding's glyph names from code 32, `.` where it has none.
const WIN_ANSI_NAMES: &str = "space exclam quotedbl numbersign dollar percent ampersand quotesingle parenleft parenright asterisk plus comma hyphen period slash \
zero one two three four five six seven eight nine colon semicolon less equal greater question \
at A B C D E F G H I J K L M N O P Q R S T U V W X Y Z bracketleft backslash bracketright asciicircum underscore \
grave a b c d e f g h i j k l m n o p q r s t u v w x y z braceleft bar braceright asciitilde . \
Euro . quotesinglbase florin quotedblbase ellipsis dagger daggerdbl circumflex perthousand Scaron guilsinglleft OE . Zcaron . \
. quoteleft quoteright quotedblleft quotedblright bullet endash emdash tilde trademark scaron guilsinglright oe . zcaron Ydieresis \
space exclamdown cent sterling currency yen brokenbar section dieresis copyright ordfeminine guillemotleft logicalnot hyphen registered macron \
degree plusminus twosuperior threesuperior acute mu paragraph periodcentered cedilla onesuperior ordmasculine guillemotright onequarter onehalf threequarters questiondown \
Agrave Aacute Acircumflex Atilde Adieresis Aring AE Ccedilla Egrave Eacute Ecircumflex Edieresis Igrave Iacute Icircumflex Idieresis \
Eth Ntilde Ograve Oacute Ocircumflex Otilde Odieresis multiply Oslash Ugrave Uacute Ucircumflex Udieresis Yacute Thorn germandbls \
agrave aacute acircumflex atilde adieresis aring ae ccedilla egrave eacute ecircumflex edieresis igrave iacute icircumflex idieresis \
eth ntilde ograve oacute ocircumflex otilde odieresis divide oslash ugrave uacute ucircumflex udieresis yacute thorn ydieresis";

/// WinAnsiEncoding's characters from 0x80 to 0x9f.
const WIN_ANSI_80: &str = "€\0‚ƒ„…†‡ˆ‰Š‹Œ\0Ž\0\0‘’“”•–—˜™š›œ\0žŸ";

/// MacRomanEncoding from 0x80.
const MAC_ROMAN_80: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»… ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{f8ff}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";

/// StandardEncoding's codes from 0xa1 that differ from nothing.
const STANDARD_HIGH: [(u8, char); 47] = [
    (0xa1, '¡'),
    (0xa2, '¢'),
    (0xa3, '£'),
    (0xa4, '⁄'),
    (0xa5, '¥'),
    (0xa6, 'ƒ'),
    (0xa7, '§'),
    (0xa8, '¤'),
    (0xa9, '\''),
    (0xaa, '“'),
    (0xab, '«'),
    (0xac, '‹'),
    (0xad, '›'),
    (0xae, 'ﬁ'),
    (0xaf, 'ﬂ'),
    (0xb1, '–'),
    (0xb2, '†'),
    (0xb3, '‡'),
    (0xb4, '·'),
    (0xb6, '¶'),
    (0xb7, '•'),
    (0xb8, '‚'),
    (0xb9, '„'),
    (0xba, '”'),
    (0xbb, '»'),
    (0xbc, '…'),
    (0xbd, '‰'),
    (0xbf, '¿'),
    (0xc1, '`'),
    (0xc2, '´'),
    (0xc3, 'ˆ'),
    (0xc4, '˜'),
    (0xc5, '¯'),
    (0xc6, '˘'),
    (0xc7, '˙'),
    (0xc8, '¨'),
    (0xca, '˚'),
    (0xcb, '¸'),
    (0xcd, '˝'),
    (0xce, '˛'),
    (0xcf, 'ˇ'),
    (0xd0, '—'),
    (0xe1, 'Æ'),
    (0xe3, 'ª'),
    (0xe8, 'Ł'),
    (0xe9, 'Ø'),
    (0xea, 'Œ'),
];
const STANDARD_HIGH_2: [(u8, char); 7] = [
    (0xeb, 'º'),
    (0xf1, 'æ'),
    (0xf5, 'ı'),
    (0xf8, 'ł'),
    (0xf9, 'ø'),
    (0xfa, 'œ'),
    (0xfb, 'ß'),
];

/// Names outside WinAnsiEncoding: StandardEncoding's, ligatures, and the
/// symbols TeX and office fonts use.
const MORE_NAMES: &[(&str, char)] = &[
    ("fi", 'ﬁ'),
    ("fl", 'ﬂ'),
    ("ff", 'ﬀ'),
    ("ffi", 'ﬃ'),
    ("ffl", 'ﬄ'),
    ("fraction", '⁄'),
    ("dotlessi", 'ı'),
    ("dotlessj", 'ȷ'),
    ("Lslash", 'Ł'),
    ("lslash", 'ł'),
    ("breve", '˘'),
    ("dotaccent", '˙'),
    ("ring", '˚'),
    ("hungarumlaut", '˝'),
    ("ogonek", '˛'),
    ("caron", 'ˇ'),
    ("minus", '−'),
    ("nbspace", '\u{a0}'),
    ("sfthyphen", '\u{ad}'),
    ("Delta", '∆'),
    ("Omega", 'Ω'),
    ("pi", 'π'),
    ("infinity", '∞'),
    ("lessequal", '≤'),
    ("greaterequal", '≥'),
    ("notequal", '≠'),
    ("approxequal", '≈'),
    ("partialdiff", '∂'),
    ("summation", '∑'),
    ("product", '∏'),
    ("integral", '∫'),
    ("radical", '√'),
    ("lozenge", '◊'),
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ε'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("rho", 'ρ'),
    ("sigma", 'σ'),
    ("tau", 'τ'),
    ("upsilon", 'υ'),
    ("phi", 'φ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
    ("Gamma", 'Γ'),
    ("Theta", 'Θ'),
    ("Lambda", 'Λ'),
    ("Xi", 'Ξ'),
    ("Pi", 'Π'),
    ("Sigma", 'Σ'),
    ("Phi", 'Φ'),
    ("Psi", 'Ψ'),
    ("arrowright", '→'),
    ("arrowleft", '←'),
    ("arrowup", '↑'),
    ("arrowdown", '↓'),
    ("arrowboth", '↔'),
    ("arrowdblright", '⇒'),
    ("element", '∈'),
    ("universal", '∀'),
    ("existential", '∃'),
    ("emptyset", '∅'),
    ("intersection", '∩'),
    ("union", '∪'),
    ("propersubset", '⊂'),
    ("reflexsubset", '⊆'),
    ("logicaland", '∧'),
    ("logicalor", '∨'),
    ("equivalence", '≡'),
    ("similar", '∼'),
    ("proportional", '∝'),
    ("asteriskmath", '∗'),
    ("bulletmath", '∙'),
    ("circlemultiply", '⊗'),
    ("circleplus", '⊕'),
    ("angleleft", '⟨'),
    ("angleright", '⟩'),
    ("prime", '′'),
    ("dotmath", '⋅'),
    ("visiblespace", '␣'),
    ("quotedblbase", '„'),
    ("quotesingle", '\''),
    ("onethird", '⅓'),
    ("twothirds", '⅔'),
    ("checkmark", '✓'),
];

fn names() -> &'static HashMap<&'static str, char> {
    static NAMES: OnceLock<HashMap<&'static str, char>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let win = win_ansi();
        let mut names: HashMap<&str, char> = MORE_NAMES.iter().copied().collect();
        for (i, name) in WIN_ANSI_NAMES.split_whitespace().enumerate() {
            if let (false, Some(c)) = (name == ".", win[32 + i]) {
                names.entry(name).or_insert(c);
            }
        }
        names
    })
}

/// A glyph name's character: a known name, `uniXXXX`, `uXXXX[XX]`, or a
/// name with a suffix (`a.sc`, `f_i`).
pub fn glyph_name(name: &str) -> Option<String> {
    if let Some(c) = names().get(name) {
        return Some(c.to_string());
    }
    if let Some(hex) = name.strip_prefix("uni")
        && hex.len() >= 4
        && hex.len() % 4 == 0
    {
        let chars: Option<String> = hex
            .as_bytes()
            .chunks(4)
            .map(|c| {
                u32::from_str_radix(std::str::from_utf8(c).ok()?, 16)
                    .ok()
                    .and_then(char::from_u32)
            })
            .collect();
        if chars.is_some() {
            return chars;
        }
    }
    if let Some(hex) = name.strip_prefix('u')
        && (4..=6).contains(&hex.len())
        && let Some(c) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
    {
        return Some(c.to_string());
    }
    if let Some((base, _)) = name.split_once('.')
        && !base.is_empty()
    {
        return glyph_name(base);
    }
    if name.contains('_') {
        return name.split('_').map(glyph_name).collect();
    }
    None
}

pub fn win_ansi() -> Encoding {
    let mut e = latin1();
    for (i, c) in WIN_ANSI_80.chars().enumerate() {
        e[0x80 + i] = (c != '\0').then_some(c);
    }
    e
}

fn latin1() -> Encoding {
    let mut e = [None; 256];
    for (code, slot) in e.iter_mut().enumerate().skip(32) {
        if code != 127 {
            *slot = char::from_u32(code as u32);
        }
    }
    e
}

pub fn mac_roman() -> Encoding {
    let mut e = latin1();
    for (i, c) in MAC_ROMAN_80.chars().enumerate() {
        e[0x80 + i] = Some(c);
    }
    e
}

pub fn standard() -> Encoding {
    let mut e = [None; 256];
    for (code, slot) in e.iter_mut().enumerate().take(127).skip(32) {
        *slot = char::from_u32(code as u32);
    }
    e[0x27] = Some('’');
    e[0x60] = Some('‘');
    for (code, c) in STANDARD_HIGH.iter().chain(&STANDARD_HIGH_2) {
        e[*code as usize] = Some(*c);
    }
    e
}

pub fn named(name: &str) -> Option<Encoding> {
    match name {
        "WinAnsiEncoding" => Some(win_ansi()),
        "MacRomanEncoding" => Some(mac_roman()),
        "StandardEncoding" => Some(standard()),
        _ => None,
    }
}

/// A Type 1 font program's own encoding, from its `dup 65 /A put` lines.
pub fn type1_builtin(program: &[u8]) -> Option<Encoding> {
    let start = program.windows(9).position(|w| w == b"/Encoding")?;
    let clear_end = program.windows(5).position(|w| w == b"eexec").unwrap_or(program.len());
    if program[start..].starts_with(b"/Encoding StandardEncoding") {
        return Some(standard());
    }
    let mut e = [None; 256];
    let mut lex = Lexer::new(&program[..clear_end], start + 9);
    let mut last: Vec<Object> = Vec::new();
    while let Some(o) = lex.object(false) {
        if o.is_keyword("readonly") || o.is_keyword("def") {
            break;
        }
        if o.is_keyword("put")
            && let [.., Object::Int(code), Object::Name(name)] = last.as_slice()
            && (0..256).contains(code)
        {
            e[*code as usize] = glyph_name(name).and_then(|s| s.chars().next());
        }
        last.push(o);
        if last.len() > 3 {
            last.remove(0);
        }
    }
    Some(e)
}

/// A ToUnicode CMap: character code, with its byte length, to text.
#[derive(Default, Debug)]
pub struct ToUnicode {
    pub map: HashMap<u32, String>,
}

impl ToUnicode {
    pub fn parse(data: &[u8]) -> ToUnicode {
        let mut map = HashMap::new();
        let mut lex = Lexer::new(data, 0);
        let mut stack: Vec<Object> = Vec::new();
        let code = |o: &Object| o.as_bytes().map(|b| b.iter().fold(0u32, |n, &x| n << 8 | x as u32));
        while let Some(o) = lex.object(false) {
            match o {
                Object::Keyword(k) if k == "endbfchar" => {
                    for pair in stack.as_chunks::<2>().0 {
                        if let (Some(c), Some(dst)) = (code(&pair[0]), pair[1].as_bytes()) {
                            map.insert(c, utf16(dst));
                        }
                    }
                    stack.clear();
                }
                Object::Keyword(k) if k == "endbfrange" => {
                    for triple in stack.as_chunks::<3>().0 {
                        let (Some(lo), Some(hi)) = (code(&triple[0]), code(&triple[1])) else {
                            continue;
                        };
                        if hi < lo || hi - lo > 0xffff {
                            continue;
                        }
                        match &triple[2] {
                            Object::String(dst) => {
                                let base: Vec<u16> = dst
                                    .chunks(2)
                                    .map(|p| u16::from_be_bytes([p[0], *p.get(1).unwrap_or(&0)]))
                                    .collect();
                                for (i, c) in (lo..=hi).enumerate() {
                                    // The last unit counts up.
                                    let mut units = base.clone();
                                    if let Some(last) = units.last_mut() {
                                        *last = last.wrapping_add(i as u16);
                                    }
                                    map.insert(c, String::from_utf16_lossy(&units));
                                }
                            }
                            Object::Array(dsts) => {
                                for (c, dst) in (lo..=hi).zip(dsts) {
                                    if let Some(d) = dst.as_bytes() {
                                        map.insert(c, utf16(d));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    stack.clear();
                }
                Object::Keyword(k) if k.starts_with("begin") => stack.clear(),
                Object::Keyword(_) => {}
                other => stack.push(other),
            }
        }
        ToUnicode { map }
    }
}

/// A code space range of a Type 0 font's encoding CMap: codes `bytes` long from low to high.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeRange {
    pub bytes: usize,
    pub low: u32,
    pub high: u32,
}

/// The code space ranges of an encoding CMap (`begincodespacerange <00> <FF> endcodespacerange`),
/// which say how many bytes each character code takes.
pub fn code_ranges(data: &[u8]) -> Vec<CodeRange> {
    let mut ranges = Vec::new();
    let mut lex = Lexer::new(data, 0);
    let mut stack: Vec<Object> = Vec::new();
    while let Some(o) = lex.object(false) {
        match o {
            Object::Keyword(k) if k == "endcodespacerange" => {
                for pair in stack.as_chunks::<2>().0 {
                    let (Some(low), Some(high)) = (pair[0].as_bytes(), pair[1].as_bytes()) else {
                        continue;
                    };
                    if low.is_empty() || low.len() != high.len() || low.len() > MAX_CODE_BYTES {
                        continue;
                    }
                    ranges.push(CodeRange {
                        bytes: low.len(),
                        low: code_of(low),
                        high: code_of(high),
                    });
                }
                stack.clear();
            }
            Object::Keyword(k) if k.starts_with("begin") => stack.clear(),
            Object::Keyword(_) => {}
            other => stack.push(other),
        }
    }
    ranges
}

/// The longest character code a CMap may have, in bytes.
pub const MAX_CODE_BYTES: usize = 4;

/// Bytes as one big-endian character code.
pub fn code_of(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |n, &b| n << 8 | b as u32)
}

fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|p| u16::from_be_bytes([p[0], *p.get(1).unwrap_or(&0)]))
        .collect();
    String::from_utf16_lossy(&units).replace('\0', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_tables() {
        assert_eq!(glyph_name("A").as_deref(), Some("A"));
        assert_eq!(glyph_name("quotedblleft").as_deref(), Some("“"));
        assert_eq!(glyph_name("fi").as_deref(), Some("ﬁ"));
        assert_eq!(glyph_name("uni00E6").as_deref(), Some("æ"));
        assert_eq!(glyph_name("a.sc").as_deref(), Some("a"));
        assert_eq!(glyph_name("f_f_i").as_deref(), Some("ffi"));
        assert_eq!(win_ansi()[0x93], Some('“'));
        assert_eq!(mac_roman()[0xbf], Some('ø'));
        assert_eq!(mac_roman()[0xff], Some('ˇ'));
        assert_eq!(standard()[0xae], Some('ﬁ'));
    }

    #[test]
    fn reads_to_unicode_maps() {
        let cmap = b"2 beginbfchar <0003> <0020> <0024> <00660069> endbfchar
            1 beginbfrange <0010> <0012> <0041> endbfrange
            1 beginbfrange <0020> <0021> [<00E6> <00F8>] endbfrange";
        let t = ToUnicode::parse(cmap);
        assert_eq!(t.map[&3], " ");
        assert_eq!(t.map[&0x24], "fi");
        assert_eq!(t.map[&0x12], "C");
        assert_eq!(t.map[&0x21], "ø");
    }

    #[test]
    fn reads_type1_encodings() {
        let program = b"/Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\ndup 65 /A put\ndup 12 /fi put\nreadonly def\ncurrentfile eexec";
        let e = type1_builtin(program).unwrap();
        assert_eq!(e[65], Some('A'));
        assert_eq!(e[12], Some('ﬁ'));
    }
}
