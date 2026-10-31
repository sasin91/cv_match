// The Jobnet extension page's button: copies the extension's folder, for "Load unpacked" to
// paste, and asks the app to open edge://extensions, which a web page can't link to.

const button = document.getElementById("open-extensions");
const status = document.getElementById("open-extensions-status");

// Copies the folder, or says to copy it by hand when the browser won't.
async function copyFolder() {
  try {
    await navigator.clipboard.writeText(button.dataset.folder);
    return "The folder is copied.";
  } catch {
    return "Copy the folder above yourself.";
  }
}

// Asks the app to open Edge's extensions page; throws the app's error when it couldn't.
async function openEdge() {
  const answer = await fetch("joblog_extension/open_edge", { method: "POST" });
  if (!answer.ok) {
    const body = await answer.json().catch(() => ({}));
    throw new Error(body.error || `The app answered ${answer.status}.`);
  }
  return "Edge is opening its extensions page.";
}

button?.addEventListener("click", async () => {
  status.classList.remove("error");
  const copied = await copyFolder();
  try {
    status.textContent = `${copied} ${await openEdge()}`;
  } catch (e) {
    console.error("joblog_extension: open_edge:", e);
    status.textContent = `${copied} ${e.message}`;
    status.classList.add("error");
  }
});
