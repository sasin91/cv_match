// The setup wizard's CV step: a PDF or text file picked is read in the browser into the CV box
// (cv_file.js, as on the match page), so only its text is posted, with the file's name.

import { readFile } from "./cv_file.js";

const picker = document.getElementById("cv_file");
const box = document.getElementById("cv_text");
const name = document.getElementById("cv_name");

picker?.addEventListener("change", async () => {
  const file = picker.files[0];
  if (!file) return;
  name.value = file.name;
  box.value = `Reading ${file.name}…`;
  try {
    box.value = await readFile(file);
  } catch (error) {
    box.value = "";
    box.placeholder = `${file.name} couldn't be read (${error.message}). Paste the CV's text here instead.`;
    console.error(error);
  }
});

// The form waits for the model, which takes up to a minute: say so on its button.
document.getElementById("cv-form")?.addEventListener("submit", (event) => {
  const button = event.target.querySelector("button[type=submit]");
  if (!button) return;
  button.disabled = true;
  button.textContent = "Reading the CV… (up to a minute)";
});
