//! A PDF's text, read in the app: the CV a user uploads, and the PDFs a job site sends. The PDF
//! is parsed by the pdf-extract crate (MIT), which reads its fonts' encodings and ToUnicode maps
//! and lays each page's text out in reading order; this module joins the pages and turns any
//! failure, a panic inside the parser included, into an error the caller says.
//!
//! It runs in a child process (child): a file that exhausts memory or the stack, or never
//! finishes, ends only that process.
//!
//! Not read: encrypted files, and text a PDF only has as glyph shapes (a scan, or a font without
//! a ToUnicode map or known encoding). Those answer an empty text, which the caller says.

pub mod child;

/// Pages are joined with this, so a page's last line and the next page's first stay apart.
const PAGE_BREAK: &str = "\n\n";

/// The PDF's text, page by page in reading order, or why it couldn't be opened.
pub fn text_of(bytes: Vec<u8>) -> Result<String, String> {
    let read = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(&bytes));
    let pages = match read {
        Ok(Ok(pages)) => pages,
        Ok(Err(e)) => return Err(format!("the PDF couldn't be read: {e}")),
        Err(_) => return Err("the PDF couldn't be read: its layout isn't one the reader understands".to_string()),
    };
    Ok(joined(&pages))
}

/// The pages' text, each tidied (tidy) and without the blank lines around it, with PAGE_BREAK
/// between them.
fn joined(pages: &[String]) -> String {
    pages
        .iter()
        .map(|page| tidy(page))
        .filter(|page| !page.is_empty())
        .collect::<Vec<_>>()
        .join(PAGE_BREAK)
}

/// page's lines without the spaces around each, and at most one blank line in a row: the reader
/// marks each gap between paragraphs with blank lines, and indents some lines.
fn tidy(page: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in page.lines().map(str::trim) {
        let repeated_blank = line.is_empty() && lines.last().is_none_or(|last| last.is_empty());
        if !repeated_blank {
            lines.push(line);
        }
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::text_of;

    /// Helvetica, as a simple font with a standard encoding.
    const HELVETICA: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";

    /// A one-page PDF with this content stream, its /Length as given, font F1 (object 5) and
    /// `extra` as more objects, with its cross reference table (with_xref).
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
        with_xref(format!(
            "%PDF-1.4\n\
             1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
             2 0 obj << /Type /Pages /Kids [{kids}] /Count 1 >> endobj\n\
             3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources {resources} /Contents 4 0 R >> endobj\n\
             4 0 obj << /Length {length} >>\nstream\n{content}\nendstream\nendobj\n\
             5 0 obj {font} endobj\n\
             {extra}\n"
        ))
    }

    /// body (the header and its objects, each starting a line) with the cross reference table,
    /// trailer and startxref a PDF ends with.
    fn with_xref(body: String) -> Vec<u8> {
        let starts = regex::Regex::new(r"(?m)^(\d+) 0 obj").unwrap();
        let mut offsets: Vec<(usize, usize)> = starts
            .captures_iter(&body)
            .map(|found| (found[1].parse().unwrap(), found.get(0).unwrap().start()))
            .collect();
        offsets.sort();
        let size = offsets.last().map_or(1, |(number, _)| number + 1);
        let mut table = format!("xref\n0 {size}\n0000000000 65535 f \n");
        for number in 1..size {
            match offsets.iter().find(|(n, _)| *n == number) {
                Some((_, offset)) => table.push_str(&format!("{offset:010} 00000 n \n")),
                None => table.push_str("0000000000 65535 f \n"),
            }
        }
        let xref_at = body.len();
        format!("{body}{table}trailer << /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").into_bytes()
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
        // Read as having no text: the stream's length can't be known.
        assert!(text_of(pdf(content, "4 0 R", "")).is_ok());
    }

    #[test]
    fn deeply_nested_arrays_are_said_to_be_unreadable_not_overflowing_the_stack() {
        let content = format!(
            "BT /F1 12 Tf 72 700 Td (Software engineer) Tj ET {}",
            "[".repeat(200_000)
        );
        assert!(text_of(pdf(&content, &content.len().to_string(), "")).is_err());
    }

    #[test]
    fn a_page_tree_pointing_back_at_itself_ends() {
        let content = "BT /F1 12 Tf 72 700 Td (Software engineer) Tj ET";
        let bytes = pdf_with(content, &content.len().to_string(), HELVETICA, "2 0 R 2 0 R 3 0 R", "");
        // The page that points back is left out; what matters is that the walk ends.
        assert!(text_of(bytes).is_ok());
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
    fn lines_lose_their_indent_and_paragraphs_keep_one_blank_line() {
        let pages = [
            "\n\n  Erfaring\n\n\n Udvikler\n".to_string(),
            " \n".to_string(),
            "Side to".to_string(),
        ];
        assert_eq!(super::joined(&pages), "Erfaring\n\nUdvikler\n\nSide to");
    }
}
