# Reading job sites through the extension (not built)

What it would take for the job scan to read Indeed (and later LinkedIn) through the user's
everyday Edge, via an extension, instead of JobFinder's own browser. It is written down, not
built: for now Indeed is read in JobFinder's own Edge shown as a normal window
(`signed_in::Showing::Window`), because Indeed's Cloudflare blocks a headless browser outright
("Blocked - Indeed.com", 2 November 2026, signed in or not).

## Why it may be needed

The window works only while the user is signed in to Windows (a scheduled scan with no desktop
has no screen to show it on), it opens on the user's screen during each scan, and a browser
driven over its DevTools port can still be told apart from a person's. If Indeed blocks the
window too, or the window is too much in the way, the extension route is what is left:

- Edge won't let a program drive the user's default profile over DevTools, so the user's own
  browser can only be used from inside it, by an extension.
- Pages read there carry the user's own browser, cookies and history, read at the user's pace.

## What the extension would do

1. **Get work from the app.** The service worker asks `GET http://127.0.0.1:8765/job_scan/pages_to_read`
   for pages the scan wants read (a search page per term, then the new posts' pages). MV3
   service workers sleep, so it is woken by `chrome.alarms` (one minute at the least) and by the
   app's scan start (the app can't call the extension; the extension asks).
2. **Open each page in a background tab** (`chrome.tabs.create({ active: false })`), one at a
   time and `PAUSE_BETWEEN_PAGES` (10 s) apart, as the signed-in browser does now.
3. **Wait for the page to show** with a content script on the site (or
   `chrome.scripting.executeScript`), then send `document.documentElement.outerHTML` and
   `location.href` to the app, as `job_scan/submit_page` does today for a page the user sends.
4. **Close the tab.** The app judges the HTML with the same readers (`indeed::listed`,
   `indeed::post_page`, and the block, check and sign-in pages), so the reading code is shared.

The app side: a queue of pages to read per scan, a wait with a time limit for each answer, and
"Edge isn't running" said in short when no answer comes. No DB change: the queue lives in memory
for the scan's time.

## In the Jobnet extension, or one of its own

| | In "Joblog autofill" (extension/jobnet-joblog) | A dedicated "JobFinder reader" extension |
|---|---|---|
| Install | Already loaded; reload once for the new permissions | A second "Load unpacked", with its own fixed ID ("key") |
| Permissions | Grows: `tabs`, `alarms`, host access to dk.indeed.com (and linkedin.com) | Only what reading needs; the joblog extension stays as it is |
| What it says it is | "Fills in Jobnet's joblog form" no longer covers it | Says exactly what it does |
| Code shared | Background fetch to the app, the base URL setting | Copied or a small shared file |

Leaning: **a dedicated extension**. Opening tabs on its own on sites the user is signed in to
is a different kind of permission from filling a form, and it should be its own switch the user
can turn off in Edge without losing the joblog autofill.

## Open questions

- Whether the user wants tabs opening in their browser every 2 hours (they are in the
  background, but show in the tab strip for the page's time). A separate minimised window
  (`chrome.windows.create({ state: "minimized" })`) keeps them out of the way.
- The sites' terms forbid reading them with programs; this is still that, at a few pages a
  scan, for the user's own job hunting.
