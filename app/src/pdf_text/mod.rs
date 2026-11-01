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

    /// A one-page PDF with this content stream, its /Length as given and `extra` as more
    /// objects. No cross reference table: the reader rebuilds it, as for any broken file.
    fn pdf(content: &str, length: &str, extra: &str) -> Vec<u8> {
        format!(
            "%PDF-1.4\n\
             1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
             2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
             3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >> endobj\n\
             4 0 obj << /Length {length} >>\nstream\n{content}\nendstream\nendobj\n\
             5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >> endobj\n\
             {extra}\n\
             trailer << /Root 1 0 R >>\n%%EOF\n"
        )
        .into_bytes()
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
}
