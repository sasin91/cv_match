// On Jobnet: a small panel next to the "Opret joblog" form that fills it in
// from a saved match in Recruiter's CV checker (cv_match/joblog/{id}, see
// modules/cv_match/Joblog.php), PDFs included. It never presses Gem: the
// person checks the form and saves it. The matches applied for but not on
// Jobnet yet come first, and when the person saves a filled form with Gem,
// the match is marked as on Jobnet in Recruiter.
//
// Jobnet's form is React, which keeps its own copy of each value and ignores
// values set from outside. So a value is set through the element's own
// prototype setter and announced with an input event, as typing would; radio
// buttons and the checkbox are clicked; a field with suggestions (company,
// postcode and town) is typed into and its matching suggestion picked (the
// job title keeps typed text, so it is the match's own); the PDFs are attached to the upload fields with a DataTransfer.

(() => {
  const api = globalThis.browser ?? globalThis.chrome;

  // Jobnet's upload fields by the PDF that goes in them.
  const UPLOADS = { application: "file-Jobansoeninger", cv: "file-CV", resume: "file-CV", job: "file-Jobannoncer" };
  // Fields that list suggestions as you type.
  const TYPEAHEADS = ["name", "postalCodeAndCity"];
  // Jobnet's date fields, typed as dd-mm-yyyy.
  const DATES = ["submissionDeadline", "submissionDate"];
  const ROOT_ID = "recruiter-joblog";
  // How long after Gem the form may take to go for it to count as saved.
  const SAVE_WINDOW_MS = 30000;

  // The match the form was filled from ({id, label}); the same with the
  // status the form had, once Gem is pressed. When the form then goes,
  // Jobnet saved it, and the match is marked as on Jobnet in Recruiter
  // (afterForm). Closing it with Luk, Fortryd or Escape saves nothing.
  let filled = null;
  let saving = null;
  // How the last marking went, for the next panel to say: [text, ok].
  let marked = null;

  document.addEventListener(
    "submit",
    (event) => {
      if (filled === null || event.target !== form()) return;
      const status = form().querySelector('input[name="searchStatus"]:checked')?.value ?? "IkkeSoegt";
      saving = { ...filled, status, at: Date.now() };
    },
    true,
  );
  document.addEventListener(
    "click",
    (event) => {
      const button = event.target.closest?.("button");
      if (button && button.type !== "submit" && /^(Luk|Fortryd)$/.test(button.textContent.trim())) saving = null;
    },
    true,
  );
  document.addEventListener("cancel", () => (saving = null), true);

  // The form appears after Jobnet's page has loaded and goes when it leaves
  // it (one page app), so the panel follows it.
  new MutationObserver(watch).observe(document.documentElement, { childList: true, subtree: true });
  watch();

  function form() {
    return document.querySelector('input[name="occupation"]')?.closest("form") ?? null;
  }

  function watch() {
    const present = document.getElementById(ROOT_ID);
    const found = form();
    if (!found) {
      present?.remove();
      if (filled !== null) afterForm();
      return;
    }
    // "Opret joblog" is a modal: it sits in the top layer above any z-index and
    // makes the rest of the page inert. So the panel goes inside the modal and
    // into the top layer itself, as a popover opened after the modal.
    const target = found.closest('dialog, [role="dialog"], [aria-modal="true"]') ?? document.body;
    if (present?.parentNode === target) return;
    const host = present ?? panel();
    target.append(host);
    if (host.showPopover && !host.matches(":popover-open")) host.showPopover();
  }

  function afterForm() {
    const saved = saving;
    filled = null;
    saving = null;
    if (!saved || Date.now() - saved.at > SAVE_WINDOW_MS) return;
    ask({ type: "logged", id: saved.id, status: saved.status }).then(
      () => (marked = [`${saved.label}: marked as on Jobnet in Recruiter.`, true]),
      (error) => (marked = [`${saved.label}: saved on Jobnet, but Recruiter wasn't told (${error.message}). Tick "entered on Jobnet" there.`, false]),
    );
  }

  // ---------- the panel ----------

  function panel() {
    const host = document.createElement("div");
    host.id = ROOT_ID;
    host.popover = "manual";
    // A shadow root keeps Jobnet's styles out of the panel and the panel's out of Jobnet.
    const shadow = host.attachShadow({ mode: "open" });
    shadow.innerHTML = `
      <style>
        :host { all: initial; }
        .box { position: fixed; right: 16px; bottom: 16px; z-index: 2147483647; width: 320px; max-width: calc(100vw - 32px);
               font: 14px/1.4 system-ui, sans-serif; color: #1d1d1b; background: #fff; border: 1px solid #d6d6cf;
               border-radius: 12px; box-shadow: 0 8px 32px rgb(0 0 0 / 0.18); padding: 12px; }
        h2 { font-size: 15px; margin: 0 0 8px; display: flex; justify-content: space-between; }
        select, button { font: inherit; width: 100%; box-sizing: border-box; padding: 8px; border-radius: 8px; border: 1px solid #d6d6cf; background: #fff; color: inherit; }
        button { margin-top: 8px; cursor: pointer; }
        button.primary { background: #2f5bd3; border-color: #2f5bd3; color: #fff; font-weight: 600; }
        button:disabled { opacity: 0.6; cursor: wait; }
        .close { width: auto; margin: 0; padding: 0 6px; border: 0; background: none; font-size: 18px; line-height: 1; }
        ul { margin: 8px 0 0; padding-left: 18px; max-height: 40vh; overflow: auto; }
        li.miss { color: #a32c2c; }
        li.ok { color: #1f7a4d; }
        p { margin: 8px 0 0; color: #6b6b66; }
      </style>
      <div class="box" role="region" aria-label="Recruiter: Jobnet joblog">
        <h2>Fill from Recruiter <button class="close" type="button" aria-label="Close">×</button></h2>
        <select aria-label="Saved match"><option>Loading your saved matches…</option></select>
        <p class="waiting" hidden></p>
        <button class="primary fill" type="button" disabled>Fill in the form</button>
        <p class="note">Check the form afterwards, then press Gem yourself.</p>
        <ul class="log" hidden></ul>
      </div>`;
    const $ = (selector) => shadow.querySelector(selector);
    $(".close").addEventListener("click", () => host.remove());
    if (marked) {
      log($(".log"), ...marked);
      marked = null;
    }

    // The ids of the ones applied for but not on Jobnet yet (Søgt or
    // Samtale, entered nowhere), listed first.
    const waiting = new Set();

    ask({ type: "matches" })
      .then(({ matches, not_joblogged }) => {
        for (const m of not_joblogged) waiting.add(String(m.id));
        const others = matches.filter((m) => !waiting.has(String(m.id)));
        if (!not_joblogged.length && !others.length) throw new Error("No saved matches yet: match a job post in Recruiter's CV checker first.");
        if (!not_joblogged.length) {
          $("select").replaceChildren(...others.map(option));
        } else {
          $("select").replaceChildren(group("Applied, not on Jobnet yet", not_joblogged), ...(others.length ? [group("Other saved matches", others)] : []));
        }
        showWaiting();
        $(".fill").disabled = false;
      })
      .catch((error) => {
        $("select").replaceChildren(new Option("Couldn't load your matches"));
        log($(".log"), error.message, false);
      });

    function option(m) {
      const item = document.createElement("option");
      item.value = m.id;
      item.textContent = [m.job_title, m.company].filter(Boolean).join(" · ") || `Match ${m.id}`;
      return item;
    }

    function group(label, matches) {
      const list = document.createElement("optgroup");
      list.label = label;
      list.append(...matches.map(option));
      return list;
    }

    function showWaiting() {
      const count = waiting.size;
      $(".waiting").textContent = count === 1 ? "1 match applied for, not on Jobnet yet." : `${count} matches applied for, not on Jobnet yet.`;
      $(".waiting").hidden = count === 0;
    }

    $(".fill").addEventListener("click", async () => {
      const button = $(".fill");
      const id = $("select").value;
      button.disabled = true;
      $("select").disabled = true;
      button.textContent = "Filling in…";
      $(".log").replaceChildren();
      filled = null;
      try {
        await fill(Number(id), (text, ok) => log($(".log"), text, ok));
        filled = { id, label: $("select").selectedOptions[0]?.textContent ?? `Match ${id}` };
        log($(".log"), "Press Gem when it's right: Recruiter then marks it as on Jobnet.", true);
      } catch (error) {
        log($(".log"), error.message, false);
      }
      $("select").disabled = false;
      button.disabled = false;
      button.textContent = "Fill in the form";
    });

    return host;
  }

  function log(list, text, ok) {
    const item = document.createElement("li");
    item.className = ok ? "ok" : "miss";
    item.textContent = text;
    list.append(item);
    list.hidden = false;
  }

  // Firefox's browser.runtime.sendMessage returns a promise and takes no
  // callback; Chrome and Edge answer through the callback.
  async function ask(message) {
    const reply = globalThis.browser
      ? await globalThis.browser.runtime.sendMessage(message)
      : await new Promise((resolve, reject) => {
          api.runtime.sendMessage(message, (answer) => {
            if (api.runtime.lastError) reject(new Error(api.runtime.lastError.message));
            else resolve(answer);
          });
        });
    if (!reply || reply.error) throw new Error(reply?.error ?? "The extension didn't answer. Reload the page and try again.");
    return reply.data;
  }

  // ---------- filling ----------

  async function fill(id, report) {
    const entry = await ask({ type: "joblog", id });
    const fields = entry.sections.flatMap((section) => section.fields);
    // Deltid first: Antal timer pr. uge only shows after it.
    fields.sort((a, b) => (a.name === "jobType" ? -1 : b.name === "jobType" ? 1 : 0));
    for (const field of fields) {
      try {
        const done = await fillField(field);
        if (done) report(`${field.label}: ${done.length > 60 ? `${done.slice(0, 60)}…` : done}`, true);
      } catch (error) {
        report(`${field.label}: ${error.message}`, false);
      }
    }
    for (const file of entry.files) {
      if (!file.ready) {
        report(`${file.label}: ${file.missing}`, false);
        continue;
      }
      try {
        const pdf = await ask({ type: "pdf", id, kind: file.kind });
        attach(UPLOADS[file.kind], pdf);
        report(`${file.label}: ${pdf.name}`, true);
      } catch (error) {
        report(`${file.label}: ${error.message}`, false);
      }
    }
  }

  // Fills one field; returns what it did, or "" when there was nothing to fill.
  async function fillField(field) {
    const { name, type, value } = field;
    if (type === "radio") {
      const radio = form().querySelector(`input[type="radio"][name="${name}"][value="${CSS.escape(value)}"]`);
      if (!radio) throw new Error(`no "${field.options?.[value] ?? value}" option on the form`);
      if (!radio.checked) radio.click();
      return field.options?.[value] ?? value;
    }
    if (type === "checkbox") {
      const box = element(name);
      if (box.checked !== Boolean(value)) box.click();
      return value ? "ticked" : "";
    }
    if (value === "") return "";
    if (TYPEAHEADS.includes(name)) return pick(element(name), value);
    // The date applied only shows once Søgt or Samtale is picked.
    const node = (await until(() => form().querySelector(`[name="${name}"]`), 1500)) ?? element(name);
    setValue(node, value);
    if (DATES.includes(name)) {
      // The date picker reads the typed date; Escape closes its calendar.
      node.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      node.blur();
    }
    if (node.value !== String(value)) throw new Error(`Jobnet didn't take "${value}"`);
    return value;
  }

  function element(name) {
    const node = form().querySelector(`[name="${name}"]`);
    if (!node) throw new Error("not on the form (hidden, or Jobnet changed it)");
    return node;
  }

  function setValue(node, value) {
    const prototype = Object.getPrototypeOf(node);
    const setter = Object.getOwnPropertyDescriptor(prototype, "value")?.set;
    node.focus();
    if (setter) setter.call(node, value);
    else node.value = value;
    node.dispatchEvent(new Event("input", { bubbles: true }));
    node.dispatchEvent(new Event("change", { bubbles: true }));
  }

  // Jobnet only keeps a company or postcode picked from its suggestions (the
  // CVR register, postcodes): typed text is cleared when the field loses
  // focus. So the value is typed, then shorter searches made from it
  // ("Nordisk Fragt A/S" finds nothing, "Nordisk Fragt" finds "Nordisk Fragt
  // ApS"), and the suggestion sharing the most words with it is picked.
  async function pick(input, value) {
    const listbox = () => document.getElementById(input.getAttribute("aria-controls") ?? "");
    let best = null;
    for (const query of searches(input.name, value)) {
      setValue(input, query);
      const options = await until(() => {
        const found = [...(listbox()?.querySelectorAll('[role="option"]') ?? [])];
        return found.length ? found : null;
      }, 2500);
      for (const option of options ?? []) {
        const score = likeness(value, option.textContent);
        if (score > (best?.score ?? 0)) best = { option, score };
      }
      if (best?.score >= 1) break;
    }
    // Fewer than half the words in common is someone else's company or town.
    // A company Jobnet doesn't know keeps its name as typed.
    if (!best || best.score < 0.5) {
      setValue(input, input.name === "name" ? value : "");
      if (input.name === "name") return `${value} (as typed: none of Jobnet's suggestions fits)`;
      throw new Error(`none of Jobnet's suggestions fits "${value}": pick one yourself`);
    }
    // The option may have gone with a later search: search for it again.
    if (!best.option.isConnected) {
      setValue(input, best.option.textContent.trim());
      const again = await until(() => [...(listbox()?.querySelectorAll('[role="option"]') ?? [])].find((o) => o.textContent.trim() === best.option.textContent.trim()), 2500);
      if (!again) throw new Error(`Jobnet's suggestion "${best.option.textContent.trim()}" went away: pick one yourself`);
      best.option = again;
    }
    const picked = best.option.textContent.trim();
    for (const type of ["mousedown", "mouseup", "click"]) best.option.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true }));
    return picked.toLowerCase() === value.toLowerCase() ? picked : `${picked} (closest to "${value}")`;
  }

  // The searches to try for a value, the whole value first.
  function searches(name, value) {
    const list = [value];
    if (name === "postalCodeAndCity") list.push(value.slice(0, 4));
    if (name === "name") list.push(value.replace(/\s+(?:a\/s|aps|i\/s|p\/s|k\/s|ivs|amba|a\.m\.b\.a\.)\.?$/i, ""), words(value).slice(0, 2).join(" "));
    return [...new Set(list.filter((query) => query.length >= 2))];
  }

  function words(text) {
    return text.toLowerCase().split(/[^\p{L}\d]+/u).filter((word) => word.length >= 3);
  }

  // How alike a suggestion is to the value: 1 when it is the value, else the
  // share of the value's words found in it ("udvikler" in "softwareudvikler").
  function likeness(value, suggestion) {
    const text = suggestion.trim().toLowerCase();
    if (text === value.trim().toLowerCase()) return 1;
    const wanted = words(value);
    if (!wanted.length) return 0;
    return (wanted.filter((word) => text.includes(word)).length / wanted.length) * 0.99;
  }

  function until(check, ms) {
    return new Promise((resolve) => {
      const started = Date.now();
      const tick = () => {
        const found = check();
        if (found || Date.now() - started > ms) resolve(found);
        else setTimeout(tick, 100);
      };
      tick();
    });
  }

  function attach(name, pdf) {
    const input = document.querySelector(`input[type="file"][name="${name}"]`);
    if (!input) throw new Error("Jobnet's upload field isn't on the page");
    const bytes = Uint8Array.from(atob(pdf.base64), (c) => c.charCodeAt(0));
    const files = new DataTransfer();
    files.items.add(new File([bytes], pdf.name, { type: "application/pdf" }));
    input.files = files.files;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  }
})();
