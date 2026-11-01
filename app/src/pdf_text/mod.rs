//! A PDF's text, read in the app: the CV a user uploads (POST match/pdf_text), which the
//! browser read with pdf.js before. Borrowed from Jonas's sappkit (examples/pdfreader): its PDF
//! syntax, cross references and object streams, filters, fonts' encodings and ToUnicode maps,
//! and its page content interpreter and reading order, with what draws (shapes, colors, images,
//! the UI) taken out.
//!
//! Not read: encrypted files, and text a PDF only has as glyph shapes (a scan, or a font without
//! a ToUnicode map or known encoding). Those answer an empty text, which the caller says.

mod document;
mod encoding;
mod font;
mod page;
mod syntax;
mod text;
mod truetype;
mod zlib;

use page::Pdf;

/// Pages are joined with this, so a page's last line and the next page's first stay apart.
const PAGE_BREAK: &str = "\n\n";

/// The PDF's text, page by page in reading order, or why it couldn't be opened.
pub fn text_of(bytes: Vec<u8>) -> Result<String, String> {
    let pdf = Pdf::open(bytes)?;
    let pages: Vec<String> = (0..pdf.page_count())
        .map(|index| text::page_text(&pdf.page(index)))
        .collect();
    Ok(pages.join(PAGE_BREAK).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::text_of;

    /// Helvetica, as a simple font with a standard encoding.
    const HELVETICA: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";

    /// A one-page PDF with this content stream, its /Length as given, font F1 (object 5) and
    /// `extra` as more objects. No cross reference table: the reader rebuilds it, as for any
    /// broken file.
    fn pdf_with(content: &str, length: &str, font: &str, kids: &str, extra: &str) -> Vec<u8> {
        pdf_with_resources(content, length, font, kids, extra, FONT_RESOURCES)
    }

    /// The page's resources: font F1.
    const FONT_RESOURCES: &str = "<< /Font << /F1 5 0 R >> >>";

    fn pdf_with_resources(
        content: &str,
        length: &str,
        font: &str,
        kids: &str,
        extra: &str,
        resources: &str,
    ) -> Vec<u8> {
        format!(
            "%PDF-1.4\n\
             1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
             2 0 obj << /Type /Pages /Kids [{kids}] /Count 1 >> endobj\n\
             3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources {resources} /Contents 4 0 R >> endobj\n\
             4 0 obj << /Length {length} >>\nstream\n{content}\nendstream\nendobj\n\
             5 0 obj {font} endobj\n\
             {extra}\n\
             trailer << /Root 1 0 R >>\n%%EOF\n"
        )
        .into_bytes()
    }

    fn pdf(content: &str, length: &str, extra: &str) -> Vec<u8> {
        pdf_with(content, length, HELVETICA, "3 0 R", extra)
    }

    fn shown(render_mode: u8) -> String {
        let content = format!("BT /F1 12 Tf {render_mode} Tr 72 700 Td (Software engineer) Tj ET");
        text_of(pdf(&content, &content.len().to_string(), "")).unwrap()
    }

    #[test]
    fn painted_text_reads() {
        assert_eq!(shown(0), "Software engineer");
    }

    #[test]
    fn a_scans_invisible_ocr_layer_reads_too() {
        assert_eq!(shown(3), "Software engineer");
    }

    #[test]
    fn a_stream_whose_length_is_itself_reads_without_recursing_forever() {
        let content = "BT /F1 12 Tf 72 700 Td (Software engineer) Tj ET";
        let text = text_of(pdf(content, "4 0 R", "")).unwrap();
        assert_eq!(text, "Software engineer");
    }

    #[test]
    fn deeply_nested_arrays_are_read_as_broken_not_overflowing_the_stack() {
        let content = format!(
            "BT /F1 12 Tf 72 700 Td (Software engineer) Tj ET {}",
            "[".repeat(200_000)
        );
        let text = text_of(pdf(&content, &content.len().to_string(), "")).unwrap();
        assert_eq!(text, "Software engineer");
    }

    #[test]
    fn a_page_tree_pointing_back_at_itself_is_walked_once() {
        let content = "BT /F1 12 Tf 72 700 Td (Software engineer) Tj ET";
        let bytes = pdf_with(content, &content.len().to_string(), HELVETICA, "2 0 R 2 0 R 3 0 R", "");
        assert_eq!(text_of(bytes).unwrap(), "Software engineer");
    }

    #[test]
    fn a_type0_font_with_one_byte_codes_reads_each_byte() {
        let cmap = "begincmap 1 begincodespacerange <00> <FF> endcodespacerange endcmap";
        let to_unicode = "begincmap 3 beginbfchar <41> <0041> <42> <0042> <43> <0043> endbfchar endcmap";
        let extra = format!(
            "6 0 obj << /Length {} >>\nstream\n{cmap}\nendstream\nendobj\n\
             7 0 obj << /Length {} >>\nstream\n{to_unicode}\nendstream\nendobj\n\
             8 0 obj << /Type /Font /Subtype /CIDFontType2 /BaseFont /Cv /DW 500 >> endobj",
            cmap.len(),
            to_unicode.len()
        );
        let font = "<< /Type /Font /Subtype /Type0 /BaseFont /Cv /Encoding 6 0 R /ToUnicode 7 0 R \
                    /DescendantFonts [8 0 R] >>";
        let content = "BT /F1 12 Tf 72 700 Td (ABC) Tj ET";
        let bytes = pdf_with(content, &content.len().to_string(), font, "3 0 R", &extra);
        assert_eq!(text_of(bytes).unwrap(), "ABC");
    }

    #[test]
    fn a_font_selected_by_a_graphics_state_reads() {
        let resources = "<< /ExtGState << /GS1 << /Font [5 0 R 12] >> >> >>";
        let content = "BT /GS1 gs 72 700 Td (Software engineer) Tj ET";
        let bytes = pdf_with_resources(content, &content.len().to_string(), HELVETICA, "3 0 R", "", resources);
        assert_eq!(text_of(bytes).unwrap(), "Software engineer");
    }

    #[test]
    fn text_a_trillion_points_out_is_read_without_trying_every_point() {
        let runs: String = (0..8)
            .map(|i| {
                format!(
                    "1 0 0 1 {} {} Tm (Run{i}) Tj ",
                    if i == 7 { 1e12 } else { 72.0 + 40.0 * i as f64 },
                    700 - 20 * i
                )
            })
            .collect();
        let content = format!("BT /F1 12 Tf {runs}ET");
        let text = text_of(pdf(&content, &content.len().to_string(), "")).unwrap();
        assert!(text.contains("Run0"), "{text}");
    }

    #[test]
    fn a_to_unicode_range_to_long_text_stops_at_the_budget() {
        let long = "0041".repeat(32_768);
        let to_unicode = format!("begincmap 1 beginbfrange <0000> <FFFF> <{long}> endbfrange endcmap");
        let map = super::encoding::ToUnicode::parse(to_unicode.as_bytes());
        let bytes: usize = map.map.values().map(String::len).sum();
        assert!(bytes <= 16 * 1024 * 1024, "{bytes}");
        assert!(!map.map.is_empty());
    }
}
