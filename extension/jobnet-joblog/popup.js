// The toolbar button's popup: sends the job post in the tab you have open to
// JobFinder, which matches it like a post its scan found (job_scan/submit_page).
// It reads the page only when you open the popup (activeTab), as the browser
// shows it with its JavaScript run, so it works on LinkedIn and Indeed while
// you're signed in, without JobFinder asking those sites for anything.

const api = globalThis.browser ?? globalThis.chrome;
// The search picked last, picked again next time.
const LAST_SEARCH = "last_search_id";

const pageLine = document.getElementById("page");
const pick = document.getElementById("pick");
const searchList = document.getElementById("search");
const send = document.getElementById("send");
const said = document.getElementById("said");

async function ask(message) {
  const answer = await api.runtime.sendMessage(message);
  if (answer?.error) throw new Error(answer.error);
  return answer?.data;
}

// The tab's address and its HTML as shown, read in the tab.
async function readTab(tab) {
  const [result] = await api.scripting.executeScript({
    target: { tabId: tab.id },
    func: () => ({ url: location.href, html: document.documentElement.outerHTML }),
  });
  return result.result;
}

async function showSearches() {
  const searches = await ask({ type: "searches" });
  if (searches.length === 0) {
    throw new Error("JobFinder has no job search to add the post to. Make one in JobFinder first (Settings, Job searches).");
  }
  const { [LAST_SEARCH]: last } = await api.storage.local.get(LAST_SEARCH);
  for (const search of searches) {
    const option = new Option(search.active ? search.name : `${search.name} (off)`, String(search.id));
    searchList.add(option);
  }
  if (searches.some((search) => String(search.id) === String(last))) searchList.value = String(last);
  pick.hidden = searches.length < 2;
}

async function start() {
  const [tab] = await api.tabs.query({ active: true, currentWindow: true });
  if (!tab?.url || !/^https?:/.test(tab.url)) {
    pageLine.textContent = "Open a job post's page first, then press this button.";
    return;
  }
  pageLine.textContent = tab.title || tab.url;
  try {
    await showSearches();
  } catch (error) {
    said.textContent = error.message;
    return;
  }
  send.disabled = false;
  send.addEventListener("click", async () => {
    send.disabled = true;
    said.textContent = "Sending…";
    try {
      const page = await readTab(tab);
      const search_id = Number(searchList.value);
      await api.storage.local.set({ [LAST_SEARCH]: search_id });
      said.textContent = await ask({ type: "send_page", search_id, url: page.url, html: page.html });
    } catch (error) {
      said.textContent = error.message;
      send.disabled = false;
    }
  });
}

start();
