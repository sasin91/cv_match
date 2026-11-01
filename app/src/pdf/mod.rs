//! Writing PDFs: a page (page::Page) laid out on A4 (layout) and written as a PDF file, in
//! Helvetica, or in DejaVu Sans with only the letters used embedded (fonts). It lays pages out
//! the way dompdf 3.1 does.

pub mod fonts;
pub mod layout;
pub mod page;

use miniz_oxide::deflate::compress_to_vec_zlib;
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use subsetter::GlyphRemapper;

use fonts::{Font, Fonts, TrueType};
use layout::{PAGE_HEIGHT, PAGE_WIDTH, Sheet};
use page::Page;

/// How hard zlib squeezes the content and font streams (its levels go 0-10; 6 is its default).
const COMPRESSION_LEVEL: u8 = 6;

/// What the PDF says made it.
const PRODUCER: &str = "JobFinder";

/// The resource names of the regular and the bold font.
const REGULAR_FONT: Name = Name(b"F1");
const BOLD_FONT: Name = Name(b"F2");

/// An embedded font's descriptor flags: symbolic, as its glyphs are reached by their ids rather
/// than through a standard encoding.
const TRUE_TYPE_FLAGS: FontFlags = FontFlags::SYMBOLIC;

/// The character collection an embedded font's glyph ids are in: its own (Identity).
const IDENTITY: SystemInfo = SystemInfo {
    registry: Str(b"Adobe"),
    ordering: Str(b"Identity"),
    supplement: 0,
};

/// The width PDF gives a font's glyphs: thousandths of an em.
const PDF_UNITS_PER_EM: f32 = 1000.0;

/// The page as a PDF file.
pub fn render(page: &Page) -> Result<Vec<u8>, String> {
    let fonts = Fonts::of(page.family)?;
    let sheets = layout::sheets(page, &fonts);
    write(&sheets, &fonts, &page.title)
}

/// Hands out the PDF's object numbers in order.
struct Refs(Ref);

impl Refs {
    fn next(&mut self) -> Ref {
        self.0.bump()
    }
}

/// How a font's text is written into the content: WinAnsi bytes for Helvetica; for an embedded
/// font two-byte glyph ids of its subset, the glyphs used so far and the letters they print.
enum Encoder<'a> {
    WinAnsi,
    Glyphs {
        font: &'a TrueType,
        remapper: GlyphRemapper,
        letters: Vec<(u16, char)>,
    },
}

impl<'a> Encoder<'a> {
    fn of(font: &'a Font) -> Self {
        match font {
            Font::Helvetica { .. } => Encoder::WinAnsi,
            Font::TrueType(font) => Encoder::Glyphs {
                font,
                remapper: GlyphRemapper::new(),
                letters: Vec::new(),
            },
        }
    }

    fn encode(&mut self, text: &str) -> Vec<u8> {
        match self {
            Encoder::WinAnsi => text.chars().map(|c| fonts::windows_1252(c).unwrap_or(b'?')).collect(),
            Encoder::Glyphs {
                font,
                remapper,
                letters,
            } => {
                let mut bytes = Vec::with_capacity(text.len() * 2);
                for c in text.chars() {
                    let glyph = font.glyph(c);
                    let id = remapper.remap(glyph.0);
                    if !letters.iter().any(|(known, _)| *known == id) {
                        letters.push((id, c));
                    }
                    bytes.extend_from_slice(&id.to_be_bytes());
                }
                bytes
            }
        }
    }
}

fn write(sheets: &[Sheet], fonts: &Fonts, title: &str) -> Result<Vec<u8>, String> {
    let mut pdf = Pdf::new();
    let mut refs = Refs(Ref::new(1));
    let catalog = refs.next();
    let tree = refs.next();
    let info = refs.next();
    let regular_ref = refs.next();
    let bold_ref = refs.next();
    let mut regular = Encoder::of(&fonts.regular);
    let mut bold = Encoder::of(&fonts.bold);

    let mut page_refs = Vec::with_capacity(sheets.len());
    for sheet in sheets {
        let page_ref = refs.next();
        let content_ref = refs.next();
        let content = sheet_content(sheet, &mut regular, &mut bold);
        pdf.stream(content_ref, &compress(&content)).filter(Filter::FlateDecode);
        let mut page = pdf.page(page_ref);
        page.media_box(Rect::new(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT))
            .parent(tree)
            .contents(content_ref);
        page.resources()
            .fonts()
            .pair(REGULAR_FONT, regular_ref)
            .pair(BOLD_FONT, bold_ref);
        page.finish();
        page_refs.push(page_ref);
    }

    pdf.catalog(catalog).pages(tree);
    let count = i32::try_from(page_refs.len()).map_err(|_| "The PDF has too many pages.".to_string())?;
    pdf.pages(tree).kids(page_refs).count(count);
    pdf.document_info(info)
        .title(TextStr(title))
        .producer(TextStr(PRODUCER));
    write_font(&mut pdf, &mut refs, regular_ref, regular, false)?;
    write_font(&mut pdf, &mut refs, bold_ref, bold, true)?;
    Ok(pdf.finish())
}

/// A sheet's lines as a content stream.
fn sheet_content<'a>(sheet: &Sheet, regular: &mut Encoder<'a>, bold: &mut Encoder<'a>) -> Vec<u8> {
    let mut content = Content::new();
    for line in sheet {
        let (name, encoder) = if line.bold {
            (BOLD_FONT, &mut *bold)
        } else {
            (REGULAR_FONT, &mut *regular)
        };
        content.begin_text();
        content.set_font(name, line.size);
        content.next_line(line.x, PAGE_HEIGHT - line.baseline);
        content.show(Str(&encoder.encode(&line.text)));
        content.end_text();
    }
    content.finish().to_vec()
}

fn compress(data: &[u8]) -> Vec<u8> {
    compress_to_vec_zlib(data, COMPRESSION_LEVEL)
}

/// Writes a font: Helvetica by name, or the embedded font's subset of the glyphs used.
fn write_font(pdf: &mut Pdf, refs: &mut Refs, id: Ref, encoder: Encoder, bold: bool) -> Result<(), String> {
    let (font, remapper, letters) = match encoder {
        Encoder::WinAnsi => {
            let name = if bold { "Helvetica-Bold" } else { "Helvetica" };
            pdf.type1_font(id)
                .base_font(Name(name.as_bytes()))
                .encoding_predefined(Name(b"WinAnsiEncoding"));
            return Ok(());
        }
        Encoder::Glyphs {
            font,
            remapper,
            letters,
        } => (font, remapper, letters),
    };
    let subset = subsetter::subset(font.data, 0, &remapper)
        .map_err(|e| format!("The font {} couldn't be embedded: {e}", font.name))?;
    let cid_ref = refs.next();
    let descriptor_ref = refs.next();
    let file_ref = refs.next();
    let unicode_ref = refs.next();
    // A subset's name starts with a tag of six capitals; the regular and the bold differ.
    let base_name = format!("{}+{}", if bold { "RCRTRB" } else { "RCRTRA" }, font.name);

    pdf.type0_font(id)
        .base_font(Name(base_name.as_bytes()))
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(cid_ref)
        .to_unicode(unicode_ref);

    let widths: Vec<f32> = remapper
        .remapped_gids()
        .map(|glyph| font.advance(ttf_parser::GlyphId(glyph)) * PDF_UNITS_PER_EM)
        .collect();
    let mut cid = pdf.cid_font(cid_ref);
    cid.subtype(CidFontType::Type2)
        .base_font(Name(base_name.as_bytes()))
        .system_info(IDENTITY)
        .font_descriptor(descriptor_ref)
        .cid_to_gid_map_predefined(Name(b"Identity"));
    cid.widths().consecutive(0, widths);
    cid.finish();

    let face = &font.face;
    let scale = PDF_UNITS_PER_EM / font.units_per_em();
    let bbox = face.global_bounding_box();
    let mut descriptor = pdf.font_descriptor(descriptor_ref);
    descriptor
        .name(Name(base_name.as_bytes()))
        .flags(TRUE_TYPE_FLAGS)
        .bbox(Rect::new(
            f32::from(bbox.x_min) * scale,
            f32::from(bbox.y_min) * scale,
            f32::from(bbox.x_max) * scale,
            f32::from(bbox.y_max) * scale,
        ))
        .italic_angle(0.0)
        .ascent(f32::from(face.ascender()) * scale)
        .descent(f32::from(face.descender()) * scale)
        .cap_height(f32::from(face.capital_height().unwrap_or(face.ascender())) * scale)
        .stem_v(stem_v(bold))
        .font_file2(file_ref);
    descriptor.finish();

    let compressed = compress(&subset);
    pdf.stream(file_ref, &compressed)
        .filter(Filter::FlateDecode)
        .pair(Name(b"Length1"), i32::try_from(subset.len()).unwrap_or(i32::MAX));

    let mut unicode = pdf_writer::types::UnicodeCmap::new(Name(b"Custom"), IDENTITY);
    for (glyph, letter) in letters {
        unicode.pair(glyph, letter);
    }
    pdf.cmap(unicode_ref, &unicode.finish());
    Ok(())
}

/// The dominant vertical stem width a reader may use for hinting, in thousandths of an em:
/// the values PDF writers commonly give a regular and a bold sans.
fn stem_v(bold: bool) -> f32 {
    /// A regular weight's stem width.
    const REGULAR_STEM_V: f32 = 80.0;
    /// A bold weight's stem width.
    const BOLD_STEM_V: f32 = 140.0;
    if bold { BOLD_STEM_V } else { REGULAR_STEM_V }
}

#[cfg(test)]
mod tests {
    use super::*;
    use page::{Family, Node, Paragraph};

    fn page(family: Family, text: &str) -> Page {
        Page {
            title: "Ansøgning: Acme".into(),
            family,
            body: vec![Node::Heading1("Jonas".into()), Node::Paragraph(Paragraph::line(text))],
        }
    }

    #[test]
    fn a_helvetica_pdf_names_its_base_fonts() {
        let pdf = render(&page(Family::Helvetica, "Kære Søren")).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/BaseFont /Helvetica-Bold"));
        assert!(text.contains("/WinAnsiEncoding"));
        assert!(!text.contains("/FontFile2"));
    }

    #[test]
    fn a_dejavu_pdf_embeds_a_small_subset() {
        let pdf = render(&page(Family::DejaVuSans, "Łódź → Kraków")).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/BaseFont /RCRTRA+DejaVuSans"));
        assert!(text.contains("/FontFile2"));
        assert!(text.contains("/ToUnicode"));
        // Only the letters used, not the whole font (about 740 kB).
        assert!(pdf.len() < 60_000, "{} bytes", pdf.len());
    }
}
