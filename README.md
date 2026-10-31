# cv_match

Recruiter's CV match as a desktop app: one program for Windows (or any desktop) that keeps your
saved matches in SQLite, serves the match page on http://127.0.0.1:8765 in your own browser, and
works with Jobnet's joblog through its browser extension. One local user, no sign-in.

| Folder | What |
| --- | --- |
| `app/` | the app (`recruiter.exe`): the matching, the pages, the database. See its README. |
| `laya-server/` | Laya on ONNX Runtime in Rust: the model the app runs in-process, and a `/v1/systemone` HTTP server |
| `assets/` | what the app builds in: the page's CSS and JS, Trongate MX, the taxonomy index and the Danish postcodes; pdf.js 4.10.38 (`match/pdfjs`) and DejaVu Sans 2.37 (`fonts`) |
| `extension/jobnet-joblog/` | the Jobnet extension the app ships and you load once in Edge or Chrome |

## Setting it up on Windows

Once, before the first build:

1. Install Rust with [rustup](https://rustup.rs) (the stable MSVC toolchain), and the Visual
   Studio Build Tools with "Desktop development with C++", which Rust's MSVC toolchain links
   with.
2. Clone this repository. Run every `cargo` command from inside `app\`: the repository root has
   no `Cargo.toml`, and cargo would pick up one in a parent folder instead.

Build it:

    cd app
    cargo build --release

The first build downloads ONNX Runtime for Laya (from cdn.pyke.io) and links it into the exe.

### Starting it

    .\target\release\recruiter.exe

On the first start the app makes an empty database, `%APPDATA%\Recruiter\recruiter.sqlite`,
writes the Jobnet extension next to it and opens http://127.0.0.1:8765/match.
Close its window to stop it. `recruiter.exe --help` lists the options.

If the app refuses, at start, a database made from an older schema, move `recruiter.sqlite`
aside and start again.

### The API key

Open http://127.0.0.1:8765/account. Pick OpenAI, Anthropic or a local server, paste the key
(a local server needs its address and a model name instead), and save it. Choose what the key is
used for: evaluating job posts, writing applications, tailoring résumés. **Check key** sends one
tiny request. The key goes in Windows Credential Manager under `Recruiter`, never in the
database. With no model named, OpenAI keys run on gpt-5.4-mini.

### Laya's model

Export the English checkpoint once, as `laya-server/README.md` ("One-time: export the model")
describes. Then put these in `%APPDATA%\Recruiter\laya`, or point `--laya` at another folder:

    laya\rl_agent_config.json   from the checkpoint folder
    laya\tokenizer\             from the checkpoint folder
    laya\laya.onnx              from export.py's out\
    laya\laya.onnx.data         from export.py's out\ (1.7 GB)

Hard links save copying the 1.7 GB file, for example
`New-Item -ItemType HardLink -Path laya\laya.onnx.data -Target <export folder>\out\laya.onnx.data`.
At start the app says whether it found the model. Without it, matches work as before with no
Laya line.

### The Jobnet extension

In Edge, open `edge://extensions`, switch on Developer mode, choose **Load unpacked**, and pick
`%APPDATA%\Recruiter\jobnet-joblog`. http://127.0.0.1:8765/joblog_extension shows the folder and
the steps. After an app update writes a newer version, press Reload on the extension.

### Your phone

Through Tailscale, without opening anything to the internet: see [docs/phone.md](docs/phone.md).
In short, run `tailscale serve --bg http://127.0.0.1:8765` once, start the app with
`--tailscale-name pc.your-tailnet.ts.net`, and open `https://pc.your-tailnet.ts.net/match` on the
phone with Tailscale on.

### Starting it at login

In Task Scheduler, create a task that runs at log on, with the action
`<repo>\app\target\release\recruiter.exe`, and arguments `--no-browser` plus any of
`--tailscale-name ...` or `--laya ...` you use.

## Trying it out

With a real key saved on the API key page, check these by hand after a change:

- [ ] **The lists:** after a few matches, http://127.0.0.1:8765/match shows them under
      Shortlist, All and the other lists. Show more loads more.
- [ ] **The free match:** paste a job post's text, with evaluating switched off on the key page.
      A match is saved and scored by the free reader.
- [ ] **The AI match:** switch evaluating on and paste another post's text. The result shows the
      requirements as the model read them, the score and, with the model in place, the Laya line.
- [ ] **Fetching a job from a link:** paste a link to a public job post (Jobindex or LinkedIn,
      say). The post is read from the page and matched. A link to this PC or the home network is
      refused.
- [ ] **Writing:** on a saved match, write the application, then the tailored résumé. Each one
      appears in its box. Write one again with notes, and the earlier version is listed under it.
- [ ] **PDFs:** Download PDF on the application, the résumé, the CV and the job post. Each opens as
      an A4 page with the expected file name.
- [ ] **Apply:** open the Apply dialog on a match. It shows the email or the employer's site,
      with the PDFs and your profile links.
- [ ] **Rejecting:** Not interested moves a match to Rejected, and Bring back returns it.
- [ ] **Jobnet:** on jobnet.dk, the extension's panel lists your matches, fills in a joblog entry
      and marks the match as on Jobnet.
- [ ] **Errors:** delete the key in Credential Manager, or switch a use off, and try a match.
      The page says why and links to the key page, and the console window logs it.
- [ ] **The phone:** with Tailscale on, the phone opens the match page and can save a match.

### The automated checks

These are the checks CI runs on Windows. Run them from `app\` before a push:

    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test --release

The parity tests compare the matching with a fixed snapshot of expected answers
(`app/tests/fixtures/parity.json`). When a change is meant to move an answer, update the
snapshot in the same commit.

`app/README.md` has more on the tests.
