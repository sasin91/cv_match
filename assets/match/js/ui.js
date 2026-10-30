// Small UI helpers for the CV match page: confirm dialogs, the success and
// error animation, toasts, counting numbers, list reordering and file drops.
//
// The dialogs and the toast stack are markup in the page (views/dialogs.php):
// these helpers fill in their text, open them and close them. They are
// native <dialog>s, so focus, Escape and the backdrop work without extra
// code. Every animation is CSS; with prefers-reduced-motion the stylesheet
// turns them off and these helpers skip the waits.

export const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const $ = (id) => document.getElementById(id);

// Shows the icon's group for `kind` and draws it again, on the next frame so
// the stroke transition runs.
function drawIcon(dialog, kind) {
  const icon = dialog.querySelector(".alert-icon");
  icon.setAttribute("class", `alert-icon ${kind}`);
  requestAnimationFrame(() => requestAnimationFrame(() => icon.classList.add("drawn")));
}

// The dialog's title and line of text; the line is hidden when there's none.
function fillText(dialog, title, text) {
  dialog.querySelector("h2").textContent = title;
  const line = dialog.querySelector(".alert-dialog-text");
  line.textContent = text;
  line.hidden = !text;
}

// Closes a dialog after its closing animation.
function closeAnimated(dialog, value) {
  if (!dialog.open || dialog.classList.contains("closing")) return;
  dialog.returnValue = value;
  if (reducedMotion()) {
    dialog.close(value);
    return;
  }
  dialog.classList.add("closing");
  dialog.addEventListener(
    "animationend",
    () => {
      dialog.classList.remove("closing");
      dialog.close(value);
    },
    { once: true },
  );
}

// Opens one of the page's dialogs. Escape and a click on the backdrop (the
// dialog element itself, outside its box) cancel; those are set up once.
function openDialog(dialog) {
  if (!dialog.dataset.ready) {
    dialog.dataset.ready = "true";
    dialog.addEventListener("cancel", (e) => {
      e.preventDefault();
      closeAnimated(dialog, "cancel");
    });
    dialog.addEventListener("click", (e) => {
      if (e.target === dialog) closeAnimated(dialog, "cancel");
    });
  }
  dialog.returnValue = "";
  dialog.showModal();
}

// Resolves with the dialog's returnValue once it has closed.
function closed(dialog) {
  return new Promise((resolve) => dialog.addEventListener("close", () => resolve(dialog.returnValue), { once: true }));
}

/**
 * Asks before something destructive or paid. Resolves true when confirmed.
 * tone "danger" colours the confirm button red.
 */
export async function confirmDialog({ title, text = "", confirm = "Continue", cancel = "Cancel", tone = "primary" }) {
  const dialog = $("confirm-dialog");
  const [no, yes] = dialog.querySelectorAll(".alert-dialog-buttons button");
  fillText(dialog, title, text);
  no.textContent = cancel;
  yes.textContent = confirm;
  yes.className = tone;
  no.onclick = () => closeAnimated(dialog, "cancel");
  yes.onclick = () => closeAnimated(dialog, "confirm");
  openDialog(dialog);
  drawIcon(dialog, tone === "danger" ? "error" : "warning");
  // The safe choice has focus, so Enter doesn't spend or delete by accident.
  no.focus();
  return (await closed(dialog)) === "confirm";
}

/**
 * The big tick (or cross) of mx-animate-success / mx-animate-error with a
 * line of text. Closes itself after MX's 1.3 seconds, or on a click.
 */
export async function flash(kind, title, text = "") {
  const dialog = $("flash-dialog");
  fillText(dialog, title, text);
  dialog.onclick = () => closeAnimated(dialog, "ok");
  openDialog(dialog);
  drawIcon(dialog, kind);
  const timer = setTimeout(() => closeAnimated(dialog, "ok"), reducedMotion() ? 900 : 1300);
  await closed(dialog);
  clearTimeout(timer);
}

/** A short note in the corner that goes away by itself. tone: "", "success" or "error". */
export function toast(text, tone = "") {
  const note = $("toast").content.firstElementChild.cloneNode(true);
  note.textContent = text;
  if (tone) note.classList.add(tone);
  $("toasts").append(note);
  setTimeout(() => {
    if (reducedMotion()) return note.remove();
    note.classList.add("leaving");
    note.addEventListener("animationend", () => note.remove(), { once: true });
  }, 3200);
}

/** Counts a number up from 0 to `to`. */
export function countUp(node, to, duration = 900) {
  if (reducedMotion()) {
    node.textContent = to;
    return;
  }
  const start = performance.now();
  const step = (now) => {
    const t = Math.min(1, (now - start) / duration);
    // Ease out cubic: fast at first, settling on the number.
    node.textContent = Math.round(to * (1 - (1 - t) ** 3));
    if (t < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

/**
 * Changes a list with `change` and slides the children that stay from their
 * old place to the new one (FLIP), so a row that moves up the ranking is seen
 * moving.
 */
export function flip(list, change) {
  const before = new Map([...list.children].map((child) => [child, child.getBoundingClientRect().top]));
  change();
  if (reducedMotion()) return;
  for (const child of list.children) {
    const top = before.get(child);
    if (top === undefined) continue;
    const moved = top - child.getBoundingClientRect().top;
    if (!moved) continue;
    child.animate([{ transform: `translateY(${moved}px)` }, { transform: "none" }], { duration: 350, easing: "cubic-bezier(.2,.8,.2,1)" });
  }
}

/** Lets a file be dropped on `zone`; `onFile` gets the first file dropped. */
export function dropZone(zone, onFile) {
  let depth = 0;
  zone.addEventListener("dragenter", (e) => {
    if (!e.dataTransfer?.types.includes("Files")) return;
    e.preventDefault();
    depth++;
    zone.classList.add("drag-over");
  });
  zone.addEventListener("dragover", (e) => {
    if (e.dataTransfer?.types.includes("Files")) e.preventDefault();
  });
  zone.addEventListener("dragleave", () => {
    if (--depth <= 0) {
      depth = 0;
      zone.classList.remove("drag-over");
    }
  });
  zone.addEventListener("drop", (e) => {
    const file = e.dataTransfer?.files?.[0];
    if (!file) return;
    e.preventDefault();
    depth = 0;
    zone.classList.remove("drag-over");
    onFile(file);
  });
}

/** Fades the nodes in one after another. */
export function reveal(nodes) {
  [...nodes].forEach((node, i) => {
    node.classList.remove("reveal");
    node.style.setProperty("--i", Math.min(i, 12));
    // Restarted when the same node is revealed again.
    void node.offsetWidth;
    node.classList.add("reveal");
  });
}
