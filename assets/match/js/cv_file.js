// Reads a picked or dropped file's text, so only text is posted: a PDF by the app
// (POST match/pdf_text), an HTML page as its visible text, anything else as it is. Shared by
// the match page (app.js) and the setup wizard's CV step (setup.js).

// match/pdf_text, from this file's place under match_module/js/.
const PDF_TEXT = new URL("../../match/pdf_text", import.meta.url);

export async function readFile(file) {
  if (!file) return "";
  if (file.type === "application/pdf" || file.name.toLowerCase().endsWith(".pdf")) return readPdf(file);
  const text = await file.text();
  if (/\.html?$/i.test(file.name)) return new DOMParser().parseFromString(text, "text/html").body.innerText;
  return text;
}

// The app answers {text}, or {error} saying why it couldn't (a scan, a broken file).
async function readPdf(file) {
  const response = await fetch(PDF_TEXT, {
    method: "POST",
    headers: { "Content-Type": "application/pdf", Accept: "application/json" },
    body: file,
  });
  const data = await response.json().catch(() => ({ error: `reading the PDF failed: ${response.status}` }));
  if (!response.ok || data.error) throw new Error(data.error ?? `reading the PDF failed: ${response.status}`);
  return data.text;
}
