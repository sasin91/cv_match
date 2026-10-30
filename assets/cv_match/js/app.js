// The CV match page: reads the CV and job post, has the server match them
// with the taxonomy, asks the language model about what the taxonomy can't
// decide, and shows the legacy ranking score. The matching and scoring are PHP (Cv_matcher.php);
// this file only reads files, calls the endpoints and puts the result the
// server prints (views/result.php) in place.
//
// Without the AI match (not signed in, or signed in without an API key of
// their own) the page runs the free match instead: one quick_match call, no
// model, nothing saved. Signed in, a post can still be fetched from a link.
// A key switched off for evaluating job posts (aiUses.job_evaluation) still
// saves the match: the server reads both texts with the free reader and
// what the taxonomy can't decide counts as missing, with no model call.
// Switched off for the résumé or application, that box says so.
//
// "Add another job post" compares several posts with the same CV: each is
// matched in turn exactly like a single post (and saved, with the AI match),
// and the jobs are listed by score. A post already matched with this CV is
// skipped and shown from its saved match, and so is a single post.
//
// ui.js shows the dialogs and toasts (their markup is views/dialogs.php)
// and runs the animations; this file decides when.

import { confirmDialog, countUp, dropZone, flash, flip, reveal, toast } from "./ui.js";

const PDFJS = "https://cdnjs.cloudflare.com/ajax/libs/pdf.js/4.10.38/";
const CV_KEY = "cv_match.cv";
// The free match keeps only the CV text, since it reads it on every match.
const CV_TEXT_KEY = "cv_match.cv_text";
// An application or résumé being written, kept for this tab so the page can
// pick it up after a reload: {id, kind, before, started}, before being
// when it was last written (null if never), so a new one is told apart.
const PENDING_KEY = "cv_match.pending";
// How long a pending one is waited for after it was started: the model call
// has 180 seconds on the server, plus a margin.
const PENDING_MS = 4 * 60 * 1000;

const $ = (id) => document.getElementById(id);
let base;
let signedIn = false;
let aiReady = false;
// use => whether the user's key (or the server's) may be used for it.
let aiUses = { resume: false, application: false, job_evaluation: false };
let maxJobs = 10;
// The saved match the page was opened on, from its link (cv_match/{id}).
let openId = null;
// Whether the page came with the user's profile as the CV (in #cv-text).
let withProfile = false;
// Job posts as read for each saved match this page made, by saved id, so a
// tailored résumé is scored on the same requirements as the CV was.
const readJobs = new Map();

export function start(baseUrl, state) {
  base = baseUrl;
  ({ signedIn, aiReady, aiUses, maxJobs, openId, withProfile } = state);
  $("add-job").addEventListener("click", addJob);
  fileInput($("cv-file"), $("cv-drop"), $("cv-text"), $("cv-file-name"));
  fileInput($("job-file"), $("job-drop"), $("job-text"), $("job-file-name"));
  for (const id of ["cv-text", "job-text"]) $(id).addEventListener("input", markSteps);
  $("match").addEventListener("click", (e) => run(match, e.currentTarget));
  $("result").addEventListener("click", filterVerdicts);
  $("batch-list").addEventListener("click", filterVerdicts);
  showJobCount();
  if (aiReady) showCv(withProfile ? profileCv() : loadCv());
  else startFree();
  markSteps();
  // A saved match's link opens it; its owner has to be signed in to see it.
  if (openId && (signedIn || aiReady)) run(async () => continuePending(await openSaved(openId)));
  else if (openId) progress("This link opens a saved match. Sign in to see it, and you'll come back to it.", true);
  if (!signedIn) return;
  watchReturn();
  $("job-fetch").addEventListener("click", (e) => run(fetchJob, e.currentTarget));
  $("job-url").addEventListener("keydown", (e) => {
    if (e.key === "Enter") run(fetchJob, $("job-fetch"));
  });
  // A link pasted into the URL field, or alone into the text box, is fetched
  // straight away.
  for (const id of ["job-url", "job-text"]) {
    $(id).addEventListener("paste", (e) => {
      const pasted = e.clipboardData.getData("text").trim();
      if (!isUrl(pasted) || (id === "job-text" && $("job-text").value.trim())) return;
      e.preventDefault();
      $("job-url").value = pasted;
      run(fetchJob, $("job-fetch"));
    });
  }
  $("cv-read")?.addEventListener("click", (e) => run(readCv, e.currentTarget));
  watchHistory();
  watchApplicationDelivery();
}

// One task at a time: every button is disabled while it runs, and the button
// that started it shows a spinner.
async function run(task, button = null) {
  for (const b of document.querySelectorAll("main button")) b.disabled = true;
  button?.classList.add("busy");
  button?.setAttribute("aria-busy", "true");
  document.body.classList.add("working");
  try {
    await task();
  } catch (error) {
    progress(error.message, true);
  } finally {
    // A button switched off for good (a purpose the key is off for) stays off.
    for (const b of document.querySelectorAll("main button")) b.disabled = b.dataset.off === "true";
    button?.classList.remove("busy");
    button?.removeAttribute("aria-busy");
    document.body.classList.remove("working");
    progressBar(null);
  }
}

function progress(text, isError = false) {
  const line = $("progress");
  line.textContent = text;
  line.classList.toggle("error", isError);
  // Shaken, so an error after a long wait is noticed.
  line.classList.remove("shake");
  if (isError) {
    void line.offsetWidth;
    line.classList.add("shake");
  }
}

// The bar under the buttons: `done` of `total` steps, indeterminate while a
// task runs without a count, hidden with null.
function progressBar(done, total = 0) {
  const bar = $("progress-bar");
  bar.hidden = done === null;
  if (done === null) return;
  bar.classList.toggle("indeterminate", !total);
  bar.style.setProperty("--done", total ? done / total : 0);
  if (total) bar.setAttribute("aria-valuenow", Math.round((done / total) * 100));
  else bar.removeAttribute("aria-valuenow");
}

// The numbered steps by the panel headings turn into ticks once filled in.
function markSteps() {
  $("cv-step").classList.toggle("done", $("cv-text").value.trim() !== "");
  $("job-step").classList.toggle("done", jobEntries().length > 0);
}

// A file picked or dropped is read into its text box.
function fileInput(input, zone, box, nameLabel) {
  const load = async (file) => {
    if (!file) return;
    nameLabel.textContent = file.name;
    zone.classList.add("loading");
    try {
      box.value = await readFile(file);
      box.dispatchEvent(new Event("input"));
      reveal([box]);
    } catch (error) {
      progress(`Couldn't read ${file.name}: ${error.message}`, true);
    } finally {
      zone.classList.remove("loading");
    }
  };
  input.addEventListener("change", (e) => load(e.target.files[0]));
  dropZone(zone, load);
  dropZone(box, load);
}

// ---------- CV ----------

function loadCv() {
  try {
    const cv = JSON.parse(localStorage.getItem(CV_KEY));
    if (cv?.profile) cv.profile = withKindNames(cv.profile);
    return cv;
  } catch {
    return null;
  }
}

// CVs read before the lists were named by term kind (titles, skills, ...)
// are still saved in some browsers: give them the new names once.
function withKindNames(profile) {
  const renamed = { titles: "title", skills: "skill", languages: "language", certifications: "certificate" };
  const out = { ...profile };
  for (const [old, kind] of Object.entries(renamed)) {
    if (old in out) {
      out[kind] ??= out[old];
      delete out[old];
    }
  }
  return out;
}

async function readCv() {
  const text = $("cv-text").value.trim();
  if (!text) throw new Error("Upload or paste a CV first.");
  progress("Reading the CV…");
  const profile = await post("extract_cv", { text });
  const cv = { text, profile, read_at: new Date().toISOString() };
  try {
    localStorage.setItem(CV_KEY, JSON.stringify(cv));
  } catch {
    // Kept for this page view only.
  }
  showCv(cv);
  progress("");
  toast(`CV read: ${profile.skill.length} skills.`, "success");
  return cv;
}

function showCv(cv) {
  $("cv-input").open = !cv && !withProfile;
  if (!cv) return;
  const p = cv.profile;
  $("cv-status").textContent = `${p.name || "CV"}: ${p.experience_years} years, ${p.skill.length} skills. Read ${new Date(cv.read_at).toLocaleDateString()}.`;
  $("cv-profile").replaceChildren(...[...p.title, ...p.skill, ...p.language, ...p.certificate].map((s) => chip(s)));
  reveal($("cv-profile").children);
  $("cv-text").value = cv.text;
  markSteps();
}

// ---------- job post from a link ----------

function isUrl(text) {
  return /^https?:\/\/\S+$/i.test(text);
}

// The server fetches the page (public addresses only) and returns its text.
async function fetchJob() {
  const url = $("job-url").value.trim();
  if (!isUrl(url)) throw new Error("Paste a link starting with http:// or https://.");
  progress("Fetching the job post…");
  const page = await fetchPage(url);
  $("job-url").value = page.url;
  $("job-text").value = page.text;
  reveal([$("job-text")]);
  markSteps();
  progress(`Read ${page.text.length.toLocaleString()} characters from ${new URL(page.url).hostname}. Check the text, then match.`);
}

function fetchPage(url) {
  return post("fetch_job", { url });
}

// ---------- the free match ----------

function startFree() {
  // Matching with the profile the page came with; nothing to restore.
  if (withProfile) return;
  let text = "";
  try {
    text = localStorage.getItem(CV_TEXT_KEY) ?? "";
  } catch {
    // No storage: the CV is pasted again next time.
  }
  $("cv-text").value = text;
  showFreeCv(text);
  markSteps();
  if (signedIn) return;
  $("job-text").addEventListener("paste", (e) => {
    if (!isUrl(e.clipboardData.getData("text").trim())) return;
    e.preventDefault();
    progress("Fetching a post from a link needs an account. Sign in, or paste the post's text here.", true);
  });
}

// A CV pasted for the free match: what it found shows after a match (showCvFound).
function showFreeCv(text) {
  $("cv-input").open = !text;
  if (text) $("cv-status").textContent = "CV added. It's read when you match.";
}

const LINKS_NEED_SIGN_IN = "Fetching a post from a link needs an account. Sign in, or paste the post's text.";

function saveCvText(cvText) {
  try {
    localStorage.setItem(CV_TEXT_KEY, cvText);
  } catch {
    // Kept for this page view only.
  }
}

async function freeMatch() {
  // Signed in, a link is fetched first, as in the AI match.
  const typed = $("job-text").value.trim();
  if (signedIn && isUrl(typed)) $("job-url").value = typed;
  if (signedIn && (isUrl(typed) || (!typed && $("job-url").value.trim()))) await fetchJob();
  const jobText = $("job-text").value.trim();
  const cvText = $("cv-text").value.trim();
  if (!cvText) throw new Error("Upload or paste a CV first.");
  if (!jobText) throw new Error("Paste or upload a job post first.");
  if (isUrl(jobText)) throw new Error(LINKS_NEED_SIGN_IN);

  saveCvText(cvText);

  progress("Matching…");
  progressBar(0);
  $("result").hidden = true;
  showResult(await requestHtml("POST", "quick_match", { job_text: jobText, cv_text: cvText }));
  $("batch-result").hidden = true;
  $("upgrade-panel").hidden = false;
  progress("");
  $("result").scrollIntoView({ behavior: "smooth" });
}

// ---------- matching ----------

async function match() {
  showMatchUrl(null);
  if (jobEntries().length > 1) return matchAll();
  progressBar(0);
  if (!aiReady) return freeMatch();

  // A link typed into the text box, or only into the URL field, is fetched first.
  const typed = $("job-text").value.trim();
  if (isUrl(typed)) $("job-url").value = typed;
  if (isUrl(typed) || (!typed && $("job-url").value.trim())) await fetchJob();

  // Checked before the CV is read, so a missing job post costs no model call.
  const text = $("job-text").value.trim();
  if (!text) throw new Error("Paste a link, the text or upload a job post first.");

  const cv = await currentCv();
  // A post already matched with this CV opens its saved match instead.
  progress("Checking your saved matches…");
  const saved = await alreadyMatched(cv, text);
  if (saved) {
    await openSaved(saved.id);
    progress("You matched this post with this CV before, so this is the saved match. Change the CV or the post to match again.");
    return;
  }
  const { job, scored } = await matchOne(text, $("job-url").value.trim(), cv, "", () => {}, null, "result");
  const savedId = showResult(scored);
  $("batch-result").hidden = true;
  progress("");
  if (savedId) readJobs.set(savedId, job);
  // Not saved: the panel says why, and so does a toast.
  const notSaved = $("result").querySelector("#documents > .error");
  if (notSaved) toast(notSaved.textContent, "error");
  if (savedId) await loadHistory();
}

// The saved CV read, when it is the profile the page came with; otherwise
// none, and the profile is read when the user matches (currentCv).
function profileCv() {
  const cached = loadCv();
  return cached?.text === $("cv-text").value.trim() ? cached : null;
}

// A CV edited or replaced since it was read is read again, so the match
// never uses the previous candidate's profile.
async function currentCv() {
  const cached = loadCv();
  const cvText = $("cv-text").value.trim();
  return cached && (!cvText || cvText === cached.text) ? cached : readCv();
}

// One job post's text through the AI match: extracted here, then matched on
// the server (match): decided by the taxonomy, judged by the model where it
// can't, scored and saved. `step` prefixes the progress lines ("Job 2 of 5: ").
// With `view` "result" or "row", `scored` is the result as the page shows it
// (views/result.php) or as a row of the ranked jobs (views/batch_row.php).
async function matchOne(text, url, cv, step = "", tick = () => {}, readJob = null, view = null) {
  progress(`${step}Reading the job post…`);
  const job = readJob ?? (await post("extract_job", { text }));

  tick();
  progress(
    aiUses.job_evaluation
      ? `${step}Matching: the taxonomy first, then the AI for what it can't decide…`
      : `${step}Matching with the taxonomy…`,
  );
  // Scored and saved in the same call, with the texts the application is written from.
  const scored = await request("POST", view === "row" ? "match?view=row" : "match", {
    job,
    profile: cv.profile,
    job_text: text,
    job_url: url,
    cv_name: cv.profile.name,
    cv_text: cv.text,
  }, view !== null);
  tick();
  if (scored.saved_id) readJobs.set(scored.saved_id, job);
  return { job, scored };
}

// ---------- several job posts ----------

// Every job post typed in, first box included, as [{text, url, box}]; a box
// holding only a link has it as the url and no text.
function jobEntries() {
  const entries = [];
  const first = $("job-text").value.trim();
  const firstUrl = $("job-url")?.value.trim() ?? "";
  if (isUrl(first)) entries.push({ text: "", url: first, box: $("job-text") });
  else if (first || firstUrl) entries.push({ text: first, url: firstUrl, box: $("job-text") });
  for (const box of document.querySelectorAll("#more-jobs textarea")) {
    const text = box.value.trim();
    if (text) entries.push(isUrl(text) ? { text: "", url: text, box } : { text, url: "", box });
  }
  return entries;
}

function addJob() {
  const count = document.querySelectorAll("#more-jobs .job-entry").length + 1;
  if (count >= maxJobs) {
    progress(`Compare up to ${maxJobs} job posts at a time.`, true);
    return;
  }
  // The box's markup is the #job-entry template in views/cv_match.php.
  const entry = $("job-entry").content.firstElementChild.cloneNode(true);
  const file = entry.querySelector("input[type=file]");
  const zone = entry.querySelector(".drop");
  const fileName = entry.querySelector(".drop-name");
  const box = entry.querySelector("textarea");
  const remove = entry.querySelector("button.remove");
  fileInput(file, zone, box, fileName);
  box.addEventListener("input", markSteps);
  remove.addEventListener("click", async () => {
    // Text typed or fetched into the box is lost with it, so that's asked first.
    if (
      box.value.trim() &&
      !(await confirmDialog({ title: "Remove this job post?", text: "Its text goes with it.", confirm: "Remove", tone: "danger" }))
    ) {
      return;
    }
    entry.classList.add("leaving");
    const done = () => {
      entry.remove();
      showJobCount();
      markSteps();
    };
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) done();
    else entry.addEventListener("animationend", done, { once: true });
  });
  $("more-jobs").append(entry);
  showJobCount();
  progress("");
  box.focus();
}

// "Job post 2", "Job post 3"… on the extra boxes, and how many are left on the button.
function showJobCount() {
  const entries = document.querySelectorAll("#more-jobs .job-entry:not(.leaving)");
  entries.forEach((entry, n) => {
    entry.querySelector(".job-entry-title").textContent = `Job post ${n + 2}`;
  });
  const count = entries.length + 1;
  $("job-count").textContent = count > 1 ? `${count}/${maxJobs}` : "";
  $("add-job").disabled = count >= maxJobs;
  $("match").querySelector(".label").textContent = count > 1 ? `Match ${count} jobs` : "Match";
}

// Each post is matched in turn against the same CV and the list redrawn as
// results come in, best first. A post that fails shows its error in the list
// and the rest carry on.
async function matchAll() {
  const entries = jobEntries();
  if (entries.length > maxJobs) throw new Error(`Compare up to ${maxJobs} job posts at a time.`);
  const cvText = $("cv-text").value.trim();
  if (!cvText && !(aiReady && loadCv())) throw new Error("Upload or paste a CV first.");
  if (!signedIn && entries.some((e) => !e.text)) throw new Error(LINKS_NEED_SIGN_IN);
  // Each post is a few model calls on the user's own key, so a batch is asked first.
  if (
    aiUses.job_evaluation &&
    !(await confirmDialog({
      title: `Match ${entries.length} job posts?`,
      text: "Each post is read and judged by the AI on your own API key, a few calls per post. Posts you matched before with this CV are skipped.",
      confirm: `Match ${entries.length} posts`,
    }))
  ) {
    return;
  }

  let cv = null;
  if (aiReady) cv = await currentCv();
  else saveCvText(cvText);

  $("result").hidden = true;
  $("upgrade-panel").hidden = true;
  $("batch-list").replaceChildren();
  const counts = { matched: 0, before: 0, failed: 0, unsaved: 0, done: 0 };
  // Three steps a post with the AI match (read, judge, score), one without.
  const perPost = aiReady ? 3 : 1;
  const total = entries.length * perPost;
  progressBar(0, total);
  for (const [n, entry] of entries.entries()) {
    const step = `Job ${n + 1} of ${entries.length}: `;
    let ticks = 0;
    const tick = () => progressBar(n * perPost + Math.min(++ticks, perPost), total);
    let row;
    try {
      let { text, url } = entry;
      if (!text) {
        progress(`${step}Fetching the job post…`);
        const page = await fetchPage(url);
        url = page.url;
        text = page.text;
        // Kept in its box, so the text can be checked and the match run again without fetching.
        entry.box.value = text;
        if (entry.box === $("job-text")) $("job-url").value = url;
      }
      let saved = null;
      if (aiReady) {
        progress(`${step}Checking your saved matches…`);
        saved = await alreadyMatched(cv, text);
      }
      if (saved) {
        row = await requestHtml("GET", `result/${saved.id}?view=row`);
        counts.before++;
      } else if (aiReady) {
        const { job, scored } = await matchOne(text, url, cv, step, tick, null, "row");
        row = scored;
        readJobs.set(Number(fragment(row).firstElementChild.dataset.savedId), job);
      } else {
        progress(`${step}Matching…`);
        row = await requestHtml("POST", "quick_match?view=row", { job_text: text, cv_text: cvText, job_url: url });
      }
      row = fragment(row).firstElementChild;
      counts.matched++;
      if (row.querySelector(".batch-documents > .error")) counts.unsaved++;
    } catch (error) {
      row = failedRow(entry.text ? firstLine(entry.text) : entry.url, error.message);
      counts.failed++;
    }
    counts.done++;
    progressBar((n + 1) * perPost, total);
    showBatch(row, counts, entries.length);
  }
  progress("");
  const notSaved = counts.unsaved ? ` ${counts.unsaved} ${counts.unsaved === 1 ? "wasn't" : "weren't"} saved.` : "";
  toast(`${counts.matched} of ${entries.length} job posts matched.${notSaved}`, counts.matched === entries.length && !counts.unsaved ? "success" : "error");
  if (!aiReady) $("upgrade-panel").hidden = false;
  if (aiReady) await loadHistory();
  $("batch-result").scrollIntoView({ behavior: "smooth" });
}

// The saved match of this CV with the same post text ({id, ...}), or null.
async function alreadyMatched(cv, text) {
  const { match } = await post("already_matched", { cv_text: cv.text, job_text: text });
  return match;
}

function firstLine(text) {
  const line = text.split("\n").find((l) => l.trim()) ?? "";
  return line.length > 80 ? `${line.slice(0, 77)}…` : line;
}

// A post that couldn't be matched, as a row (template#batch-failed) with why.
function failedRow(label, error) {
  const row = $("batch-failed").content.firstElementChild.cloneNode(true);
  row.querySelector("strong").textContent = label || "Job post";
  row.querySelector(".error").textContent = error;
  return row;
}

// A row the server printed (views/batch_row.php), or a failed one, put in
// the ranking: rows already there slide to their new place (FLIP) and the
// new one fades in. The note under the heading counts how it went.
function showBatch(row, counts, total) {
  $("batch-result").hidden = false;
  $("batch-note").textContent =
    `${counts.matched} of ${total} matched` +
    (counts.before ? ` (${counts.before} matched before, shown from your saved matches)` : "") +
    (counts.failed ? `, ${counts.failed} couldn't be matched` : "") +
    (counts.done < total ? ". Still working…" : ".") +
    (aiReady ? " Each match is saved with your other matches." : "");
  localDates(row);
  showCvFound(row.querySelector("template.cv-found"));
  const documents = row.querySelector(".batch-documents");
  if (documents) wireDocuments(documents);
  const list = $("batch-list");
  flip(list, () => {
    list.append(row);
    const ranked = [...list.children].sort((a, b) => Number(b.dataset.score) - Number(a.dataset.score));
    list.replaceChildren(...ranked);
  });
  reveal([row]);
}

// ---------- result ----------

// The result as the server prints it (views/result.php) put in #result: the
// ring fills and the score counts up, the requirements fade in, a saved
// match's buttons are wired and the address bar follows it. Returns the
// saved match's id, or null.
function showResult(html) {
  const result = $("result");
  result.replaceChildren(fragment(html));
  result.hidden = false;
  const ring = result.querySelector(".score-ring");
  const percent = Number(ring.dataset.percent);
  ring.style.setProperty("--value", 0);
  requestAnimationFrame(() => requestAnimationFrame(() => ring.style.setProperty("--value", percent)));
  countUp(result.querySelector("#score-value"), percent);
  localDates(result);
  showCvFound(result.querySelector("template.cv-found"));
  const documents = result.querySelector("#documents");
  if (documents) wireDocuments(documents);
  reveal(result.querySelectorAll(".summary, .apply-notice, .group, .soft-skills, #application-panel"));
  const id = Number(result.querySelector(".match-result").dataset.savedId) || null;
  showMatchUrl(id);
  return id;
}

// What the free match found in the CV (template.cv-found in its result), in
// the CV panel.
function showCvFound(found) {
  if (!found) return;
  $("cv-input").open = false;
  $("cv-status").textContent = found.dataset.status;
  $("cv-profile").replaceChildren(found.content);
  reveal($("cv-profile").children);
}

// The result's filter: the buttons above the requirements show only the
// missing, partial or met ones.
function filterVerdicts(e) {
  const button = e.target.closest(".filter button[data-show]");
  if (!button) return;
  const list = button.parentElement.nextElementSibling;
  const show = button.dataset.show;
  for (const b of button.parentElement.children) b.setAttribute("aria-pressed", String(b === button));
  list.dataset.show = show;
  reveal(list.querySelectorAll(show === "all" ? ".group > ul > .item" : `.group > ul > .item.${show}`));
}

// ---------- saved matches ----------

// The list comes with the page, best first, a batch at a time: the "Show
// more" row loads the next batch when it scrolls into view (or is clicked).
// Each row is a link to the match's own page, so ctrl/cmd-click and
// middle-click open it in a new tab; a plain click opens it here instead.
// Trongate MX polls cv_match/history_list (the first batch) into the hidden
// #history-latest every 30 seconds while the page is in view, so matches
// saved elsewhere (a job search, a tool posting to its API) show up on their
// own. Coming back to the page (another tab, a phone waking, the
// back/forward cache) reads it at once. cvMatchHistory puts an answer in
// the list when it changed, revealing the new rows.
let shownHistory = "";

function watchHistory() {
  shownHistory = $("history").innerHTML;
  localDates($("history"));
  reveal($("history").children);
  $("history").addEventListener("click", (e) => {
    const link = e.target.closest("a[data-id]");
    if (!link || e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    // While a task runs the rows are off, as the page's buttons are (run()).
    if (!document.body.classList.contains("working")) run(() => openSaved(Number(link.dataset.id)), link);
  });
  watchMoreHistory();
  // On phones the lists are a select: picking one opens it, as its tab would.
  $("history-list-select").addEventListener("change", (e) => {
    location.href = new URL(e.target.value, base).href;
  });
  window.cvMatchHistory = showHistory;
  let readAt = 0;
  const readOnReturn = () => {
    if (Date.now() - readAt < 2000) return;
    readAt = Date.now();
    loadHistory();
  };
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) TrongateMX.stopPolling($("history-latest"));
    else readOnReturn();
  });
  window.addEventListener("pageshow", (e) => {
    if (e.persisted && !document.hidden) readOnReturn();
  });
  // Trongate MX starts the polling once the page has loaded; a page opened
  // in a background tab waits until it's looked at.
  document.addEventListener("DOMContentLoaded", () => {
    if (document.hidden) TrongateMX.stopPolling($("history-latest"));
  });
}

const moreHistory = new IntersectionObserver((entries) => {
  for (const entry of entries) {
    if (entry.isIntersecting) loadMoreHistory(entry.target);
  }
}, { rootMargin: "600px 0px" });

function watchMoreHistory() {
  const link = $("history").querySelector("a.more[data-more]");
  if (!link || link.dataset.watched) return;
  link.dataset.watched = "1";
  link.addEventListener("click", (e) => {
    e.preventDefault();
    loadMoreHistory(link);
  });
  moreHistory.observe(link);
}

// The next batch in place of the Show more row, with its own Show more.
// Without JS the link opens the page from that batch.
async function loadMoreHistory(link) {
  if (link.dataset.loading) return;
  link.dataset.loading = "1";
  link.textContent = "Loading…";
  try {
    const batch = fragment(await requestHtml("GET", link.dataset.more.replace(/^cv_match\//, "")));
    // Not rows: the sign-in page, most likely.
    if (batch.firstElementChild?.tagName !== "LI") throw new Error("No rows");
    // A match shows once, even when a batch is loaded twice.
    for (const row of batch.querySelectorAll("li[id]")) {
      if (document.getElementById(row.id)) row.remove();
    }
    localDates(batch);
    moreHistory.unobserve(link);
    const rows = [...batch.children];
    link.closest("li").replaceWith(batch);
    reveal(rows);
    watchMoreHistory();
  } catch {
    delete link.dataset.loading;
    link.textContent = "Couldn't load more. Try again";
  }
}

// The list read now (MX takes a click while it isn't polling), then polled
// again. The answer arrives in cvMatchHistory.
function loadHistory() {
  const poller = $("history-latest");
  if (!poller) return;
  TrongateMX.stopPolling(poller);
  poller.click();
  TrongateMX.startPolling(poller, "30s");
}

// The server writes dates in its own time zone; a match saved just after
// midnight here is shown on the day it was saved here. A <time> marked
// data-format="datetime" gets the time of day too.
function localDates(list) {
  for (const time of list.querySelectorAll("time[datetime]")) {
    const at = new Date(time.dateTime);
    time.textContent = time.dataset.format === "datetime" ? at.toLocaleString() : at.toLocaleDateString();
  }
}

// A changed answer becomes the list, with its own Show more: rows loaded
// below are dropped and load again in order as the list is scrolled.
// Keeping them would keep a cursor from before the change, which skips a
// new match that ranks above it.
function showHistory() {
  const poller = $("history-latest");
  const latest = poller.querySelector(".history-latest ul");
  poller.replaceChildren();
  if (!latest || latest.innerHTML === shownHistory) return;
  shownHistory = latest.innerHTML;
  const list = $("history");
  const fresh = [...latest.querySelectorAll(":scope > li[id]")].filter((row) => !document.getElementById(row.id));
  list.replaceChildren(...latest.children);
  watchMoreHistory();
  localDates(list);
  $("history-empty").hidden = list.querySelector("li[id]") !== null;
  reveal(fresh);
}

// A saved match shown like a fresh one, from its stored verdicts
// (cv_match/result/{id}), with its post back in the job box.
async function openSaved(id) {
  progress("Opening the saved match…");
  showResult(await requestHtml("GET", `result/${id}`));
  $("batch-result").hidden = true;
  const job = $("result").querySelector("template.match-job");
  if ($("job-url")) $("job-url").value = job.dataset.url;
  $("job-text").value = job.content.textContent;
  markSteps();
  progress("");
  $("result").scrollIntoView({ behavior: "smooth" });
  return id;
}

// The address bar follows the match on show: cv_match/{id} for a saved match,
// so it can be copied and linked to, plain cv_match otherwise. Replaced, not
// pushed, so Back still leaves the page.
function showMatchUrl(id) {
  const url = new URL(id ? `cv_match/${id}` : "cv_match", base);
  // The list asked for (?list=) stays, so a reload shows the same list.
  url.search = location.search;
  if (url.pathname !== location.pathname) history.replaceState(null, "", url);
}

// ---------- job application and tailored résumé ----------

// What can be written for a saved match, from the CV and the post.
const DOCUMENTS = {
  application: {
    noun: "application",
    write: "Write application",
    endpoint: "write_application",
    busy: "Writing the application…",
    done: "Application written",
  },
  resume: {
    noun: "résumé",
    write: "Tailor résumé",
    endpoint: "tailor_resume",
    busy: "Tailoring the résumé…",
    done: "Résumé tailored",
  },
};
const BOTH_DONE = "Résumé tailored and application written";

// ---------- Apply ----------

// The Apply dialog (#application-delivery in views/cv_match.php): its
// content is a fragment the server prints (views/application_delivery.php),
// read on every opening so a newly written application counts: how the post
// wants the application delivered, by email or on the employer's site, with
// fields to copy and the PDFs to attach or upload. The body is the
// application box's text, edits included: the box next to the button (the
// open match's, or a batch row's), whose boxes the dialog's PDF buttons use too.
let deliveryDocuments = null;

function wireApply(button, id) {
  button.addEventListener("click", () =>
    run(async () => {
      const response = await fetch(new URL(`cv_match/application_delivery/${id}`, base));
      if (!response.headers.get("Content-Type")?.startsWith("text/html")) {
        const data = await response.json().catch(() => ({}));
        throw new Error(data.error ?? `The Apply dialog couldn't be opened: ${response.status}`);
      }
      $("application-delivery-body").replaceChildren(fragment(await response.text()));
      deliveryDocuments = documentsOf(button);
      const box = deliveryDocuments.querySelector('.document[data-kind="application"] textarea');
      if (box && !box.hidden && box.value.trim()) $("delivery-body").value = box.value;
      $("application-delivery").showModal();
    }, button),
  );
}

// The dialog's copy and PDF buttons, and a click on the backdrop closing it.
function watchApplicationDelivery() {
  const dialog = $("application-delivery");
  if (!dialog) return;
  dialog.addEventListener("click", (e) => {
    if (e.target === dialog) return dialog.close();
    const copy = e.target.closest("button[data-copy]");
    if (copy) return copyText($(copy.dataset.copy), $("application-delivery-status"));
    const pdf = e.target.closest("button[data-pdf]");
    if (pdf) downloadMatchPdf(Number(dialog.querySelector(".application-delivery").dataset.id), pdf.dataset.pdf, pdf, deliveryDocuments);
  });
}

// ---------- Jobnet joblog ----------

// The match as an entry for Jobnet's joblog ("Opret joblog"), as the server
// prints it (views/joblog_panel.php): every field in Jobnet's order and
// words, each with its own copy button, the three PDFs Jobnet keeps, the AI
// prompt and whether it's in the candidate's joblog. Read when opened, and
// again on every opening, so a newly written application counts.
function wireJoblog(box) {
  const id = Number(box.dataset.id);
  const body = box.querySelector(".joblog-body");
  const shown = (template) => body.replaceChildren(box.querySelector(template).content.cloneNode(true));
  box.addEventListener("toggle", () => {
    if (!box.open) return;
    shown("template.joblog-loading");
    requestHtml("GET", `joblog_panel/${id}`)
      .then(async (html) => {
        body.replaceChildren(fragment(html));
        localDates(body);
        await withCvrAddress(body);
        reveal([body]);
      })
      .catch((error) => {
        shown("template.joblog-failed");
        body.querySelector(".error").textContent = error.message;
      });
  });
  body.addEventListener("input", (e) => {
    const left = e.target.closest(".joblog-field")?.querySelector(".left");
    if (left) left.textContent = String(e.target.maxLength - e.target.value.length);
  });
  body.addEventListener("click", (e) => {
    const copy = e.target.closest("button.copy-field");
    if (copy) return copyText(copy.parentElement.querySelector("input, textarea"), copy.closest(".joblog-field").querySelector(".status"));
    const pdf = e.target.closest("button.joblog-pdf");
    if (pdf) return downloadMatchPdf(id, pdf.dataset.kind, pdf, documentsOf(pdf));
    const choice = e.target.closest(".joblog-options button.chip");
    if (choice) return chooseStatus(id, body, choice);
    const prompt = e.target.closest("button.joblog-prompt");
    if (prompt) copyJoblogPrompt(body, prompt.closest(".joblog-section"));
  });
  body.addEventListener("change", async (e) => {
    const logged = e.target.closest(".joblog-logged input");
    if (logged && !(await saveJoblog(id, body, chosenStatus(body), logged.checked))) logged.checked = !logged.checked;
  });
}

// How far the candidate is with the job, picked on its field and saved with
// the match; the chip stays as it was if saving fails.
async function chooseStatus(id, body, chip) {
  if (!(await saveJoblog(id, body, chip.dataset.value, body.querySelector(".joblog-logged input").checked))) return;
  for (const other of chip.parentElement.children) {
    other.classList.toggle("chosen", other === chip);
    other.setAttribute("aria-pressed", String(other === chip));
  }
}

function chosenStatus(body) {
  return body.querySelector('.joblog-field[data-name="searchStatus"] .chip.chosen')?.dataset.value ?? "IkkeSoegt";
}

// What the candidate tells Recruiter about the entry (cv_match/joblog_status):
// the status and whether it's in their Jobnet joblog. The server answers the
// "In your Jobnet joblog" section with when (views/joblog_logged.php), and
// the day applied for the date field, which follows the status.
async function saveJoblog(id, body, status, logged) {
  try {
    const section = fragment(await requestHtml("POST", "joblog_status", { id, status, logged })).firstElementChild;
    localDates(section);
    body.querySelector(".joblog-saved").replaceWith(section);
    const applied = body.querySelector('.joblog-field[data-name="submissionDate"] input');
    if (applied) applied.value = section.dataset.appliedOn ?? "";
    loadHistory();
    return true;
  } catch (error) {
    toast(`Couldn't save it: ${error.message}`, "error");
    return false;
  }
}

// When the post names no postcode and town, the company's address in the
// CVR register (cvrapi.dk) fills them in, marked as such: it is the
// registered address, which may not be where the job is. The browser asks
// cvrapi.dk itself, as company sign-up does, so lookups count against the
// visitor's own address, not the server's quota. Only a company whose name
// shares most words with the post's is taken; a failed lookup changes nothing.
const CVRAPI = "https://cvrapi.dk/api";
// A slow register must not hold up the joblog: after this, it is shown without.
const CVR_TIMEOUT_MS = 4000;
// "A/S", "ApS" and the like at the end of a name: the register writes them its own way.
const LEGAL_FORM = /\s+(?:a\/s|aps|i\/s|p\/s|k\/s|ivs|amba|a\.m\.b\.a\.)\.?$/i;

async function withCvrAddress(body) {
  const field = (name) => body.querySelector(`.joblog-field[data-name="${name}"]`);
  const input = (name) => field(name)?.querySelector("input");
  const company = input("name")?.value.trim() ?? "";
  if (!input("postalCodeAndCity") || input("postalCodeAndCity").value !== "" || company === "") return;
  let found;
  try {
    const response = await fetch(`${CVRAPI}?${new URLSearchParams({ search: company.replace(LEGAL_FORM, ""), country: "dk" })}`, { headers: { Accept: "application/json" }, signal: AbortSignal.timeout(CVR_TIMEOUT_MS) });
    found = await response.json();
  } catch {
    return;
  }
  if (found?.error || !found?.zipcode || !found?.city || !sameCompany(company, String(found.name ?? ""))) return;
  const note = `From the CVR register (${found.name}): the registered address, check that it's where the job is`;
  const fill = (name, value) => {
    input(name).value = value;
    const hint = field(name).querySelector(".hint");
    hint.textContent = note;
    hint.hidden = false;
  };
  fill("postalCodeAndCity", `${found.zipcode} ${found.city}`);
  if (input("companyAddress")?.value === "" && found.address) fill("companyAddress", found.address);
  if (input("companyCountry")?.value === "") input("companyCountry").value = "Danmark";
  for (const chip of field("unknownPostalOrCity")?.querySelectorAll(".chip") ?? []) chip.classList.toggle("chosen", chip.dataset.value === "no");
}

// Whether a register name is the post's company: most of the post's words
// in it, legal forms aside ("Nordisk Fragt A/S" and "NORDISK FRAGT ApS").
function sameCompany(wanted, name) {
  const words = (text) => text.toLowerCase().split(/[^\p{L}\d]+/u).filter((word) => word.length >= 2 && !["as", "aps", "is", "ps", "ks", "ivs", "amba", "a", "s"].includes(word));
  const want = words(wanted);
  const have = new Set(words(name));
  return want.length > 0 && want.filter((word) => have.has(word)).length / want.length >= 0.5;
}

// The fallback for when no extension fills the form: a prompt for an AI that
// drives the browser (Claude in Chrome and the like) with every value as the
// panel shows it now, edits included: a typed field's text, or what a
// choice's chosen option says to do (data-prompt). It stops before saving,
// and leaves the PDFs to the candidate, since such an agent can't pick files
// on the computer. Shown once copied, so it can be read, and copied by hand
// if the clipboard refuses.
function copyJoblogPrompt(body, section) {
  const lines = [];
  for (const part of body.querySelectorAll(".joblog-section")) {
    const fields = part.querySelectorAll(".joblog-field");
    if (!fields.length) continue;
    lines.push("", `${part.querySelector("h5").textContent}:`);
    for (const field of fields) {
      const label = field.dataset.label;
      const value = field.querySelector("input, textarea")?.value.trim() ?? field.querySelector(".chip.chosen")?.dataset.prompt ?? "";
      if (value === "") lines.push(`- ${label}: leave empty`);
      else if (value.includes("\n")) lines.push(`- ${label}:`, ...value.split("\n").map((line) => `    ${line}`));
      else lines.push(`- ${label}: ${value}`);
    }
  }
  const area = section.querySelector(".joblog-prompt-text");
  area.value = [
    "Fill in a new entry in my joblog on Jobnet (jobnet.dk, \"Opret joblog\"). I am signed in. Use exactly these values, field by field, in Jobnet's order.",
    "Stilling eller arbejdsområde, Virksomhedens navn and Postnummer og by only keep a value picked from Jobnet's suggestions; typed text is cleared. Type the value and pick the matching suggestion; if none shows, search for a shorter part (\"Backend\" for \"Backend-udvikler\" finds \"Softwareudvikler, backend\"), pick the closest and tell me which you picked.",
    "Antal timer pr. uge only shows for Deltid. Ansøgningsfrist is typed as dd-mm-yyyy.",
    ...lines,
    "",
    "Don't upload any files: I'll add the application, CV and job ad PDFs myself. Don't press Gem: stop when the form is filled and tell me what you couldn't fill.",
  ].join("\n");
  area.hidden = false;
  copyText(area, section.querySelector(".status"));
}

// A saved match's application and résumé panel, as the server printed it
// (views/documents.php) in the result or a batch row: its boxes, the Apply
// button and the Jobnet joblog, which is read when it is opened.
function wireDocuments(panel) {
  const notes = panel.querySelector(".notes textarea");
  for (const box of panel.querySelectorAll(".document")) wireDocument(box, notes);
  const apply = panel.querySelector("button.apply");
  if (apply) wireApply(apply, Number(apply.dataset.id));
  const joblog = panel.querySelector("details.joblog");
  if (joblog) wireJoblog(joblog);
}

// A document box's buttons: Write (asked first when it replaces a text),
// Copy and Download PDF. Its kind, saved match, when it was last written and
// whether the résumé was written with fields (structured) are on the box;
// the text as written is the textarea's default value. Writing answers the
// box as the server prints it with the new text (views/document_box.php),
// which takes this one's place. A structured résumé
// gets its PDF laid out from the fields, so edits to the box aren't in it;
// the page says so first. Copy sits next to Write, above the text, so it's in
// view on a phone.
function wireDocument(box, notes) {
  const kind = box.dataset.kind;
  const id = Number(box.dataset.id);
  const doc = DOCUMENTS[kind];
  const write = box.querySelector("button.write");
  const copy = box.querySelector("button.copy");
  const pdf = box.querySelector("button.download-pdf");
  const status = box.querySelector(".status");
  const text = box.querySelector("textarea");
  const written = text.defaultValue;
  const structured = box.dataset.structured === "true";
  write.addEventListener("click", async () => {
    // Writing again replaces the text, edits included, and costs another model call.
    if (
      !text.hidden &&
      text.value.trim() &&
      !(await confirmDialog({
        title: `Write the ${doc.noun} again?`,
        text: `The new one replaces the text below, your edits included, and uses your API key.`,
        confirm: "Write it again",
      }))
    ) {
      return;
    }
    run(async () => {
      const panel = box.parentElement;
      const fresh = await writeDocument(box, notes);
      const both = kind === "resume" && (await afterResume(panel, notes, id, fresh));
      progress("");
      await loadHistory();
      await flash("success", both ? BOTH_DONE : doc.done);
    }, write);
  });
  copy.addEventListener("click", () => copyText(text, status));
  pdf.addEventListener("click", async () => {
    if (
      structured &&
      text.value.trim() !== written.trim() &&
      !(await confirmDialog({
        title: "Your edits aren't in the PDF",
        text: "The PDF is made from the written résumé, so the changes you made in the box aren't in it. Add them to the notes and write it again, or download the PDF without them.",
        confirm: "Download without them",
      }))
    ) {
      return;
    }
    downloadPdf(kind, id, text, pdf);
  });
}

// Writes a document box's text and puts the box the server answers in its
// place; returns that box. pendingToo goes into the pending state with it.
async function writeDocument(box, notes, pendingToo = {}) {
  const kind = box.dataset.kind;
  const id = Number(box.dataset.id);
  const doc = DOCUMENTS[kind];
  progress(doc.busy);
  progressBar(0);
  setPending({ id, kind, before: box.dataset.writtenAt ?? null, started: Date.now(), ...pendingToo });
  let fresh;
  try {
    fresh = fragment(await requestHtml("POST", doc.endpoint, { id, notes: notes.value.trim() })).firstElementChild;
  } catch (error) {
    // A dropped connection (phone locked, tab in the background) leaves
    // it pending: the server finishes it and the page picks it up.
    if (!(error instanceof TypeError)) clearPending();
    throw error;
  }
  box.replaceWith(fresh);
  wireDocument(fresh, notes);
  reveal(fresh.querySelectorAll(".keywords, textarea"));
  clearPending();
  return fresh;
}

// Tailoring the résumé writes the application with it, so both are ready
// to apply with: only when there's none yet (one written, maybe edited,
// stays) and the key may write it.
// Whether it wrote it.
async function writeApplicationToo(panel, notes) {
  const box = panel.querySelector('.document[data-kind="application"]');
  if (!box || box.dataset.writtenAt || box.querySelector("button.write").dataset.off === "true") return false;
  // A reload while it's written still compares the résumé (continuePending).
  await writeDocument(box, notes, { afterResume: true });
  return true;
}

// After the résumé is tailored: the application it brings (writeApplicationToo),
// then the résumé's own match against the post, next to the CV's
// (compareResume). Each runs when the other fails, so neither has to be
// written again to get it; whether the application was written.
async function afterResume(panel, notes, id, resumeBox) {
  try {
    return await writeApplicationToo(panel, notes);
  } finally {
    await compareResume(id, resumeBox.querySelector("textarea").value, resumeBox.querySelector(".compare"));
  }
}

// The tailored résumé matched against the same post as a new saved match,
// shown next to the original CV's score. The post as read for the original
// match is reused when this page still has it, so the two are scored on the
// same requirements; otherwise it is read again.
// The line with both scores is printed by the server
// (views/resume_comparison.php), with a button opening the résumé's match.
async function compareResume(id, resumeText, line) {
  progress("Matching the tailored résumé against the post…");
  const saved = await request("GET", `saved/${id}`);
  // Already matched when the page was reloaded after it.
  let resumeId = (await post("already_matched", { cv_text: resumeText, job_text: saved.job_text })).match?.id;
  if (!resumeId) {
    const job = readJobs.get(id) ?? (await post("extract_job", { text: saved.job_text }));
    const profile = await post("extract_cv", { text: resumeText });
    const cv = { text: resumeText, profile: { ...profile, name: `${profile.name || saved.cv_name || "CV"} (tailored résumé)` } };
    const { scored } = await matchOne(saved.job_text, saved.job_url ?? "", cv, "Tailored résumé: ", () => {}, job);
    resumeId = scored.saved_id;
    if (!resumeId) throw new Error(scored.save_error || "The tailored résumé was matched, but its match wasn't saved.");
  }
  line.replaceChildren(fragment(await requestHtml("GET", `resume_comparison/${id}/${resumeId}`)));
  line.hidden = false;
  const open = line.querySelector("button.open-saved");
  open.addEventListener("click", () => run(() => openSaved(Number(open.dataset.id)), open));
}

// ---------- coming back to a pending application or résumé ----------

// Writing takes a while, and a phone that is locked or switches apps
// meanwhile drops the request. The server finishes and saves it anyway, so
// when the page comes back while one is pending it reloads on the saved
// match and continues there (continuePending).
function watchReturn() {
  let hiddenAt = null;
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      hiddenAt = Date.now();
      return;
    }
    const away = hiddenAt === null ? 0 : Date.now() - hiddenAt;
    hiddenAt = null;
    if (pending() && away > 3000) reloadOnPending();
  });
  // Restored from the back/forward cache: its request is gone. A button
  // switched off for the key may be on again by now (the account page is
  // where its link goes), so the page is read again; the address is the
  // saved match's own.
  window.addEventListener("pageshow", (e) => {
    if (!e.persisted) return;
    if (pending()) reloadOnPending();
    else if (document.querySelector('main button[data-off="true"]')) location.reload();
  });
}

function reloadOnPending() {
  history.replaceState(null, "", new URL(`cv_match/${pending().id}`, base));
  location.reload();
}

// After the page opened a saved match: if an application or résumé for it
// is pending, scroll to it and wait until the server has saved it, then show
// it (with the application a tailored résumé brings, and its match against the post).
async function continuePending(id) {
  const p = pending();
  if (!p || p.id !== id) return;
  const doc = DOCUMENTS[p.kind];
  const box = () => $("documents").querySelector(`.document[data-kind="${p.kind}"]`);
  box()?.scrollIntoView({ behavior: "smooth", block: "start" });
  let saved = await request("GET", `saved/${id}`);
  let waited = false;
  const written = (match) => match[`${p.kind}_written_at`] && String(match[`${p.kind}_written_at`]) !== String(p.before);
  while (!written(saved)) {
    if (Date.now() - p.started > PENDING_MS) {
      clearPending();
      throw new Error(`The ${doc.noun} didn't come back from the model. Press ${doc.write} to try again.`);
    }
    progress(`Still ${doc.busy.charAt(0).toLowerCase()}${doc.busy.slice(1)} It carries on if you leave the page.`);
    await new Promise((resolve) => setTimeout(resolve, 5000));
    saved = await request("GET", `saved/${id}`);
    waited = true;
  }
  // Written meanwhile: the match is shown again, with it.
  if (waited) {
    showResult(await requestHtml("GET", `result/${id}`));
    box()?.scrollIntoView({ behavior: "smooth", block: "start" });
  }
  clearPending();
  const documents = $("documents");
  let both = false;
  if (p.kind === "resume") both = await afterResume(documents, documents.querySelector(".notes textarea"), saved.id, box());
  // The application Tailor résumé brought: the résumé's comparison is still to come.
  else if (p.afterResume) {
    const resumeBox = documents.querySelector('.document[data-kind="resume"]');
    await compareResume(saved.id, resumeBox.querySelector("textarea").value, resumeBox.querySelector(".compare"));
    both = true;
  }
  progress("");
  await loadHistory();
  await flash("success", both ? BOTH_DONE : doc.done);
}

function pending() {
  try {
    return JSON.parse(sessionStorage.getItem(PENDING_KEY) ?? "null");
  } catch {
    return null;
  }
}

function setPending(p) {
  try {
    sessionStorage.setItem(PENDING_KEY, JSON.stringify(p));
  } catch {
    // Without storage the page just can't pick it up after a reload.
  }
}

function clearPending() {
  try {
    sessionStorage.removeItem(PENDING_KEY);
  } catch {}
}

// One of a saved match's PDFs from outside its box (the joblog, the Apply
// dialog); a button marked unavailable says why it can't be had yet. The
// box's own PDF button is used when it shows, so the text is the box's,
// edits included, and a tailored résumé warns when the edits won't be in it.
function downloadMatchPdf(id, kind, button, documents) {
  if (button.classList.contains("unavailable")) return toast(button.title, "error");
  const own = documents?.querySelector(`.document[data-kind="${kind}"] button.download-pdf`);
  if (own && !own.hidden) return own.click();
  downloadPdf(kind, id, { value: "" }, button);
}

// The application and résumé boxes a node sits next to: the open match's
// (#documents) or a batch row's (.batch-documents).
function documentsOf(node) {
  return node.closest("#documents, .batch-documents");
}

// The textarea's current text as a PDF, edits included, saved as a download.
function downloadPdf(kind, id, area, button) {
  run(async () => {
    const response = await fetch(new URL("cv_match/pdf", base), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, kind, text: area.value }),
    });
    if (!response.ok || !response.headers.get("Content-Type")?.startsWith("application/pdf")) {
      const data = await response.json().catch(() => ({}));
      throw new Error(data.error ?? `The PDF couldn't be made: ${response.status}`);
    }
    const name = /filename\*=UTF-8''([^;]+)/.exec(response.headers.get("Content-Disposition") ?? "");
    // The page's hidden download link (#pdf-download), pointed at the PDF and clicked.
    const link = $("pdf-download");
    link.href = URL.createObjectURL(await response.blob());
    link.download = name ? decodeURIComponent(name[1]) : `${kind}.pdf`;
    link.click();
    const href = link.href;
    setTimeout(() => URL.revokeObjectURL(href), 60000);
    toast("PDF downloaded.", "success");
  }, button);
}

// The textarea's current text, so edits made on the page are what gets copied.
async function copyText(area, status) {
  status.classList.remove("error");
  try {
    await navigator.clipboard.writeText(area.value);
  } catch {
    // Older browsers or a page without clipboard permission: copy the
    // selection. setSelectionRange because select() alone selects nothing
    // on iOS.
    area.focus();
    area.setSelectionRange(0, area.value.length);
    let copied = false;
    try {
      copied = document.execCommand("copy");
    } catch {
      // Not supported either.
    }
    if (!copied) {
      status.textContent = "Couldn't copy. The text is selected: copy it yourself.";
      status.classList.add("error");
      return;
    }
  }
  status.textContent = "";
  toast("Copied to clipboard.", "success");
}

function chip(text) {
  return fromTemplate("chip", text);
}

// A copy of a one-element <template> in the page, with `text` in it.
function fromTemplate(id, text) {
  const node = $(id).content.firstElementChild.cloneNode(true);
  node.textContent = text;
  return node;
}

// ---------- io ----------

function post(method, body) {
  return request("POST", method, body);
}

function requestHtml(verb, method, body) {
  return request(verb, method, body, true);
}

// Markup the server printed, ready to put in place.
function fragment(html) {
  const template = document.createElement("template"); // server HTML
  template.innerHTML = html; // server HTML
  return template.content;
}

// The endpoint's JSON answer; with `html`, the HTML it prints as a string. A
// failure answers JSON {error} either way, thrown here.
async function request(verb, method, body, html = false) {
  const response = await fetch(new URL(`cv_match/${method}`, base), {
    method: verb,
    headers: { ...(body === undefined ? {} : { "Content-Type": "application/json" }), Accept: html ? "text/html" : "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (html && response.ok && response.headers.get("Content-Type")?.startsWith("text/html")) return response.text();
  const data = await response.json().catch(() => ({ error: `${method} failed: ${response.status}` }));
  if (!response.ok || data.error) throw new Error(data.error ?? `${method} failed: ${response.status}`);
  return data;
}

async function readFile(file) {
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
