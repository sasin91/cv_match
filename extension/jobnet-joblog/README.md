# Recruiter: Jobnet joblog

A browser extension that fills in Jobnet's "Opret joblog" form from a saved
match in Recruiter's CV checker: the same values the match's "Jobnet joblog"
panel shows (`match/joblog/{id}`, app/src/matches/joblog.rs), and the
application, CV and job post as PDFs in Jobnet's upload fields. It never
presses Gem: check the form and save it yourself.

One folder for Chrome, Edge and Firefox (Manifest V3). No build step.

## Install

It talks to the Recruiter desktop app (`app/` in this repo) on this computer,
http://127.0.0.1:8765/, which ships this folder: the app's "Jobnet extension"
page says where it wrote it and how to load it in Edge or Chrome ("Load
unpacked"). The `key` in manifest.json fixes the extension's ID
(jijhihfppobcoconjjllmffbohpffbll), which is how the app knows the changes it
sends come from this extension. Firefox gives each install its own ID, so it
works with the website only.

For the website instead, set `base` in the extension's storage to
`https://recruiter.trongate.dev/` and sign in to Recruiter in the same browser:
the extension uses that sign-in.

From the browser stores: the links are on https://recruiter.trongate.dev/joblog_extension,
and the candidate's pages show a banner with the one for your browser until the extension is installed.

From this folder (development, or before a store has it):

- **Edge**: `edge://extensions`, turn on Developer mode, "Load unpacked", pick
  this folder.
- **Chrome**: `chrome://extensions`, turn on Developer mode, "Load unpacked",
  pick this folder.
- **Firefox** (140 or later): `about:debugging#/runtime/this-firefox`, "Load
  Temporary Add-on…", pick `manifest.json` (it stays until Firefox restarts).

## Use

Open Jobnet, Joblog, "Opret joblog". A "Fill from Recruiter" box shows in the
bottom right corner: pick the saved match and press "Fill in the form". The box
lists what it filled and what it couldn't.

The matches you have applied for but not entered on Jobnet yet (status Søgt or
Samtale in the match's joblog panel, "entered on Jobnet" not ticked: the list
"Applied, not on Jobnet yet" on Recruiter) come first in the box, with how many
there are. When you press Gem on a form the box filled in and Jobnet saves it
(the form closes), the box ticks "entered on Jobnet" for that match in
Recruiter, with the status the form had, so it leaves that list. Closing the
form with Luk or Fortryd marks nothing.

The job title is the match's own, typed in as it is. Jobnet only keeps a
company or postcode picked from its own suggestions (the CVR register,
postcodes) and clears typed text. So for those the extension searches with the
value, then with parts of it, and picks the suggestion sharing the most words
with it: "Nordisk Fragt A/S" becomes "Nordisk Fragt ApS". The box says when it
picked something other than the value itself. When nothing fits, the company
keeps its name as typed and the postcode is left empty.

"Hvilken dato har du søgt jobbet?" (only asked at Søgt or Samtale) gets the day
you set that status in Recruiter.

## How it works

- `content.js` runs on jobnet.dk. Jobnet's form is React, which ignores values
  set from outside, so each value is set through the element's own prototype
  setter and announced with an input event, as typing would; radio buttons and
  the checkbox are clicked; PDFs are attached to the upload fields with a
  DataTransfer.
- `recruiter.js` runs on Recruiter's own pages and only sets
  `data-jobnet-joblog="<version>"` on the page, so its "add the extension"
  banner stays hidden.
- `background.js` fetches from Recruiter (a content script can't fetch from
  another site): the saved matches (`match/history`), the joblog entry and
  the PDFs (`match/pdf`), with your Recruiter cookie, and marks a match as
  entered on Jobnet (`match/joblog_status`).

To point it at another copy of Recruiter (a dev machine), set `base` in the
extension's storage, e.g. from its service worker console:
`chrome.storage.local.set({ base: "http://recruiter.test/" })`, and add that
address to `host_permissions`.

## Publishing

`make extension` (in the repository root) builds `build/jobnet-joblog-<version>.zip`,
the file each store takes: the manifest, the scripts and `icons/`. Bump
`version` in manifest.json for every upload. `store/` holds what the stores
ask for besides the zip: the 440x280 promo tile (Chrome), the 300x300 logo
(Edge) and `listing.md`, the texts for the store forms. The privacy policy is
the "Privacy policy" part of the web app's /joblog_extension page; change it
with the manifest.
