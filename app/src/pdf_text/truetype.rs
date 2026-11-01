//! The text behind a symbolic TrueType font's codes, read from the font
//! program itself: codes to glyphs through its `cmap`, glyphs to text
//! through a Unicode `cmap` or the glyph names in `post`.

use std::collections::HashMap;

use super::encoding::glyph_name;

/// The Macintosh glyph order, which `post` tables index into.
const MAC_NAMES: &str = ".notdef .null nonmarkingreturn space exclam quotedbl numbersign dollar percent ampersand quotesingle \
parenleft parenright asterisk plus comma hyphen period slash zero one two three four five six seven eight nine colon semicolon \
less equal greater question at A B C D E F G H I J K L M N O P Q R S T U V W X Y Z bracketleft backslash bracketright \
asciicircum underscore grave a b c d e f g h i j k l m n o p q r s t u v w x y z braceleft bar braceright asciitilde \
Adieresis Aring Ccedilla Eacute Ntilde Odieresis Udieresis aacute agrave acircumflex adieresis atilde aring ccedilla eacute \
egrave ecircumflex edieresis iacute igrave icircumflex idieresis ntilde oacute ograve ocircumflex odieresis otilde uacute \
ugrave ucircumflex udieresis dagger degree cent sterling section bullet paragraph germandbls registered copyright trademark \
acute dieresis notequal AE Oslash infinity plusminus lessequal greaterequal yen mu partialdiff summation product pi integral \
ordfeminine ordmasculine Omega ae oslash questiondown exclamdown logicalnot radical florin approxequal Delta guillemotleft \
guillemotright ellipsis nbspace Agrave Atilde Otilde OE oe endash emdash quotedblleft quotedblright quoteleft quoteright \
divide lozenge ydieresis Ydieresis fraction currency guilsinglleft guilsinglright fi fl daggerdbl periodcentered \
quotesinglbase quotedblbase perthousand Acircumflex Ecircumflex Aacute Edieresis Egrave Iacute Icircumflex Idieresis Igrave \
Oacute Ocircumflex apple Ograve Uacute Ucircumflex Ugrave dotlessi circumflex tilde macron breve dotaccent ring cedilla \
hungarumlaut ogonek caron Lslash lslash Scaron scaron Zcaron zcaron brokenbar Eth eth Yacute yacute Thorn thorn minus \
multiply onesuperior twosuperior threesuperior onehalf onequarter threequarters franc Gbreve gbreve Idotaccent Scedilla \
scedilla Cacute cacute Ccaron ccaron dcroat";

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(at)?, *d.get(at + 1)?]))
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *d.get(at)?,
        *d.get(at + 1)?,
        *d.get(at + 2)?,
        *d.get(at + 3)?,
    ]))
}

fn table<'a>(font: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    table_in(font, 0, tag)
}

/// A table of the font whose table directory starts at `base`: 0 for a
/// single font, the font's offset in a collection.
fn table_in<'a>(font: &'a [u8], base: usize, tag: &[u8; 4]) -> Option<&'a [u8]> {
    let count = u16_at(font, base + 4)? as usize;
    (0..count).find_map(|i| {
        let record = base + 12 + 16 * i;
        if font.get(record..record + 4)? != tag {
            return None;
        }
        let (offset, length) = (u32_at(font, record + 8)? as usize, u32_at(font, record + 12)? as usize);
        font.get(offset..offset.checked_add(length)?)
    })
}

/// One `cmap` subtable: character to glyph.
fn subtable(cmap: &[u8], at: usize) -> HashMap<u32, u16> {
    let mut out = HashMap::new();
    let Some(format) = u16_at(cmap, at) else {
        return out;
    };
    match format {
        0 => {
            for code in 0..256 {
                if let Some(&g) = cmap.get(at + 6 + code) {
                    out.insert(code as u32, g as u16);
                }
            }
        }
        4 => {
            let segments = u16_at(cmap, at + 6).unwrap_or(0) as usize / 2;
            let ends = at + 14;
            let starts = ends + 2 * segments + 2;
            let deltas = starts + 2 * segments;
            let ranges = deltas + 2 * segments;
            for s in 0..segments {
                let (Some(end), Some(start), Some(delta), Some(range)) = (
                    u16_at(cmap, ends + 2 * s),
                    u16_at(cmap, starts + 2 * s),
                    u16_at(cmap, deltas + 2 * s),
                    u16_at(cmap, ranges + 2 * s),
                ) else {
                    break;
                };
                for c in start..=end.min(0xfffe) {
                    let glyph = if range == 0 {
                        c.wrapping_add(delta)
                    } else {
                        let at = ranges + 2 * s + range as usize + 2 * (c - start) as usize;
                        match u16_at(cmap, at) {
                            Some(0) | None => 0,
                            Some(g) => g.wrapping_add(delta),
                        }
                    };
                    if glyph != 0 {
                        out.insert(c as u32, glyph);
                    }
                }
            }
        }
        6 => {
            let (first, count) = (
                u16_at(cmap, at + 6).unwrap_or(0) as u32,
                u16_at(cmap, at + 8).unwrap_or(0) as usize,
            );
            for i in 0..count {
                if let Some(g) = u16_at(cmap, at + 10 + 2 * i) {
                    out.insert(first + i as u32, g);
                }
            }
        }
        12 => {
            let groups = u32_at(cmap, at + 12).unwrap_or(0) as usize;
            for i in 0..groups.min(1 << 16) {
                let g = at + 16 + 12 * i;
                let (Some(start), Some(end), Some(glyph)) = (u32_at(cmap, g), u32_at(cmap, g + 4), u32_at(cmap, g + 8))
                else {
                    break;
                };
                for c in start..=end.min(start + 0xffff) {
                    out.insert(c, (glyph + c - start) as u16);
                }
            }
        }
        _ => {}
    }
    out
}

/// Text for each one byte code of a symbolic TrueType font. Codes the
/// font gives no text for are left out; a font with neither a Unicode
/// `cmap` nor glyph names gives an empty map.
pub fn code_text(font: &[u8]) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    let Some(cmap) = table(font, b"cmap") else {
        return out;
    };
    let mut symbol = None;
    let mut mac = None;
    let mut glyph_text: HashMap<u16, String> = HashMap::new();
    for i in 0..u16_at(cmap, 2).unwrap_or(0) as usize {
        let (Some(platform), Some(encoding), Some(offset)) = (
            u16_at(cmap, 4 + 8 * i),
            u16_at(cmap, 6 + 8 * i),
            u32_at(cmap, 8 + 8 * i),
        ) else {
            break;
        };
        let map = subtable(cmap, offset as usize);
        match (platform, encoding) {
            (3, 0) => symbol = Some(map),
            (1, 0) => mac = Some(map),
            (0, _) | (3, 1) | (3, 10) => {
                for (c, g) in map {
                    if let Some(c) = char::from_u32(c) {
                        glyph_text.entry(g).or_insert_with(|| c.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(post) = table(font, b"post")
        && u32_at(post, 0) == Some(0x0002_0000)
    {
        let count = u16_at(post, 32).unwrap_or(0) as usize;
        let mac_names: Vec<&str> = MAC_NAMES.split_whitespace().collect();
        // Custom names follow the index array, as Pascal strings.
        let mut custom = vec![];
        let mut at = 34 + 2 * count;
        while let Some(&len) = post.get(at) {
            let Some(name) = post.get(at + 1..at + 1 + len as usize) else {
                break;
            };
            custom.push(String::from_utf8_lossy(name).into_owned());
            at += 1 + len as usize;
        }
        for g in 0..count {
            let index = u16_at(post, 34 + 2 * g).unwrap_or(0) as usize;
            let name = if index < 258 {
                mac_names.get(index).map(|n| n.to_string())
            } else {
                custom.get(index - 258).cloned()
            };
            if let Some(text) = name.as_deref().and_then(glyph_name) {
                glyph_text.entry(g as u16).or_insert(text);
            }
        }
    }
    for code in 0u32..256 {
        // A (3, 0) table maps codes into the private use area.
        let glyph = symbol
            .as_ref()
            .and_then(|m| {
                [code, 0xf000 + code, 0xf100 + code, 0xf200 + code]
                    .iter()
                    .find_map(|c| m.get(c).copied())
            })
            .or_else(|| mac.as_ref().and_then(|m| m.get(&code).copied()));
        if let Some(text) = glyph.and_then(|g| glyph_text.get(&g)) {
            out.insert(code, text.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn mac_names_are_the_258() {
        assert_eq!(super::MAC_NAMES.split_whitespace().count(), 258);
    }
}
