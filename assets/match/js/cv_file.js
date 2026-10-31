// Reads a picked or dropped file's text in the browser, so only text is posted: a PDF with
// pdf.js, an HTML page as its visible text, anything else as it is. Shared by the match page
// (app.js) and the setup wizard's CV step (setup.js).

// pdf.js 4.10.38 (Mozilla, Apache 2.0), which reads an uploaded CV's text: served by the
// desktop app from its own files (match/pdfjs), so uploads work offline.
const PDFJS = new URL("../pdfjs/", import.meta.url).href;

export async function readFile(file) {
  if (!file) return "";
  if (file.type === "application/pdf" || file.name.toLowerCase().endsWith(".pdf")) return readPdf(file);
  const text = await file.text();
  if (/\.html?$/i.test(file.name)) return new DOMParser().parseFromString(text, "text/html").body.innerText;
  return text;
}

async function readPdf(file) {
  const pdfjs = await import(`${PDFJS}pdf.min.mjs`);
  pdfjs.GlobalWorkerOptions.workerSrc = `${PDFJS}pdf.worker.min.mjs`;
  const pdf = await pdfjs.getDocument({ data: await file.arrayBuffer() }).promise;
  const pages = [];
  for (let n = 1; n <= pdf.numPages; n++) {
    const content = await (await pdf.getPage(n)).getTextContent();
    let line = "";
    const lines = [];
    for (const item of content.items) {
      line += item.str;
      if (item.hasEOL) {
        lines.push(line);
        line = "";
      }
    }
    lines.push(line);
    pages.push(lines.join("\n"));
  }
  return pages.join("\n\n").replace(/\n{3,}/g, "\n\n").trim();
}
