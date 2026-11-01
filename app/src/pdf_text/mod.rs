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
