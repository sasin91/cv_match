# cv_match

Recruiter's CV match as a desktop app: one program for Windows or Linux that keeps your
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
writes the Jobnet extension next to it and opens the dashboard (http://127.0.0.1:8765/match). It
has an alert while no AI model is in use, and a row each for the AI model, your profile, Laya, the
job scan and the Jobnet extension, saying what is set up and linking to what is missing. Under them
are how many matches each list holds and the shortlist's best three, then the match form.

The links go to the setup (http://127.0.0.1:8765/setup), whose steps are:

1. where the database is;
2. an AI model: this app's own llama.cpp model when the PC's graphics card has room for a good
   one (Qwen3 4B or bigger, at least half of it on the card), else an OpenAI key (gpt-5.4-mini)
   or a Claude key; the steps are /llama_setup and /account, which come back to the setup while
   the database has no profile or saved matches;
3. your CV, read by that model into your profile, which the match page matches with;
4. how often the job scan runs by itself: a Task Scheduler task, `Recruiter\Job scan`, running
   `recruiter.exe scan` (the scan itself isn't ported yet, so for now a run only says so).

Close its window to stop it. `recruiter.exe --help` lists the options.

If the app refuses, at start, a database made from an older schema, move `recruiter.sqlite`
aside and start again.

### The API key

Open http://127.0.0.1:8765/account. Pick OpenAI, Anthropic or a local server, paste the key
(a local server needs its address and a model name instead), and save it. One key is kept per
provider, so an OpenAI and an Anthropic key can be saved side by side. Under **Use my keys for**,
pick the key each use runs on: evaluating job posts, writing applications, tailoring résumés.
**Check key** sends one tiny request. The keys go in Windows Credential Manager under
`Recruiter`, never in the database. With no model named, OpenAI keys run on gpt-5.4-mini.

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

### The app's own model (llama.cpp)

Instead of a key, the app can run a GGUF model itself. Open http://127.0.0.1:8765/llama_setup (or
**Set it up** on the API key page):

1. **Download llama.cpp** fetches the latest release's Windows build for this computer's card
   (CUDA for NVIDIA, Vulkan for every other card, CPU without one) and unzips it into
   `%APPDATA%\Recruiter\llama`, or the folder `--llama` names. A `llama-server.exe` already on
   the PATH (winget's llama.cpp, say) is used instead, and nothing is downloaded.
2. Each offered model (Qwen3 1.7B to 14B) is weighed against this computer's memory and graphics
   card. **Download** the recommended one into `llama\models`.
3. **Use this model** saves it as **This app's own model (llama.cpp)**, and the API key page
   picks what it is used for.

You can also put llama.cpp and `.gguf` files there by hand:

    llama\llama-server.exe      and the DLLs from the same zip
    llama\models\*.gguf         for example Qwen3-8B-Q4_K_M.gguf

On the API key page pick **This app's own model (llama.cpp)**. With the model field blank the app
picks the biggest model that fits the graphics card's memory with room for its context, or splits
one between the card and memory; the page lists each model and how it fits this computer. The
server starts on the first AI call (on a free port on 127.0.0.1), writes `llama\llama-server.log`,
and stops when the app does. An NVIDIA or AMD card without CUDA, HIP or Vulkan in the folder is
said on the page instead of quietly running on the CPU.

### The Jobnet extension

Open http://127.0.0.1:8765/joblog_extension and press **Copy the folder and open Edge's
extensions**. It copies `%APPDATA%\Recruiter\jobnet-joblog` and opens `edge://extensions`. There,
switch on Developer mode, choose **Load unpacked**, paste the folder and press Select Folder. After an app update writes a newer version, press Reload on the extension.

### Your phone

Through Tailscale, without opening anything to the internet: see [docs/phone.md](docs/phone.md).
In short, run `tailscale serve --bg http://127.0.0.1:8765` once, start the app with
`--tailscale-name pc.your-tailnet.ts.net`, and open `https://pc.your-tailnet.ts.net/match` on the
phone with Tailscale on.

### Installing it

`recruiter.exe install` (from a build folder, say), or double-clicking `Recruiter-setup.exe` from a
GitHub release (the same program under another name; pushing a `v*` tag makes the release),
copies the program to `%LOCALAPPDATA%\Programs\Recruiter`, adds it to the Start menu and to
Windows' installed apps, points the job scan's task at the copy when there is one, and starts the
copy, which opens the setup wizard. It needs no administrator. Installing again replaces the
program and keeps your data, which is how an update goes in; close Recruiter first.

Uninstall it from Windows' installed apps, or with `recruiter.exe uninstall`. It removes the
program, the shortcut and the job scan's task, and keeps the database, the models and the API
keys.

### On Linux (and WSL)

Build it the same way (`cargo build --release` in `app/`), or take `Recruiter-x86_64.AppImage` from
a release. `recruiter install` (or running `Recruiter-setup.AppImage`) copies the program, or the
AppImage, to `~/.local/lib/recruiter/recruiter`, links `~/.local/bin/recruiter` to it and adds a
Recruiter launcher to the applications menu, which starts it in a terminal. The database is
`~/.local/share/Recruiter/recruiter.sqlite`, and API keys go in the keyring (the Secret Service:
GNOME Keyring or KWallet), which has to be running to save one.

The job scan's schedule is a systemd user timer, `recruiter-scan.timer`, in
`~/.config/systemd/user` (`systemctl --user list-timers` shows it). Without systemd (WSL with
systemd off, say) the setup says it couldn't be scheduled. On WSL, turn systemd on in
`/etc/wsl.conf` (`[boot]` `systemd=true`); an AppImage there may need
`--appimage-extract-and-run` when FUSE is missing. `recruiter uninstall` removes the program, the
link, the launcher and the timer, and keeps the data. macOS isn't covered yet.

### Starting it at login

In Task Scheduler, create a task that runs at log on, with the action
`<repo>\app\target\release\recruiter.exe`, and arguments `--no-browser` plus any of
`--tailscale-name ...`, `--laya ...` or `--llama ...` you use.

## Developing

One command, on Windows and Linux alike, builds the app, starts it, rebuilds and restarts it on
every change, and reloads the page in the browser:

    cd app
    cargo dev

It needs [watchexec](https://github.com/watchexec/watchexec) once:
`cargo install watchexec-cli --locked`.

- `cargo dev` is an alias (`app/.cargo/config.toml`) for the small program in `app/xtask/`, the
  usual Rust stand-in for npm or Composer scripts. It runs watchexec, which runs a debug build
  (`cargo run -- --dev --no-browser`), and it opens http://127.0.0.1:8765/match once the first
  build is up.
- A change to `src/`, `templates/`, `queries/`, `migrations/`, `.sqlx/`, `Cargo.toml`,
  `../assets/`, `../extension/` or `../laya-server/src/` restarts it, and the open page reloads
  itself as soon as the new run answers. A CSS or JS change needs no compiling, since debug
  builds read `../assets/` from disk, so it shows in a second or two; a Rust or template change
  takes as long as the build.
- `--dev` puts a small script on each page that asks `/dev/started` every half second which run
  of the app it is, and reloads the page when that changes. Release builds refuse `--dev` and
  never send the script.
- It uses your real database unless you give it another, for example
  `cargo dev --database $env:TEMP\recruiter-dev.sqlite` (or `/tmp/recruiter-dev.sqlite` on
  Linux). Other `recruiter` options go on to the app the same way, and `--port 8766` runs it
  beside a running release build (the Jobnet extension only talks to 8765).

Stop it with Ctrl+C.

## Trying it out

With a real key saved on the API key page, check these by hand after a change:

- [ ] **Installing:** copy `recruiter.exe` to `Recruiter-setup.exe` (or take it from a release)
      and double-click it. It says what it did and a new window starts Recruiter from
      `%LOCALAPPDATA%\Programs\Recruiter`, opening /setup. Recruiter is in the Start menu and in
      Settings > Apps > Installed apps; with a job scan task, its action runs the installed copy.
      Uninstall it there: the window lists what was removed and waits for Enter, the folder goes
      a few seconds later, and `%APPDATA%\Recruiter` stays.
- [ ] **The dashboard and the setup:** start with `--database` naming a new file. The browser
      opens /match with the "No AI model is set up" alert and the missing rows; Set one up opens
      /setup, which suggests the app's own model on a PC with a good graphics card, else an OpenAI key.
      Set one up and land back on /setup; read a PDF CV into the profile; pick how often to check
      for new jobs, and `Recruiter\Job scan` shows in Task Scheduler (run it once: no console
      window opens). Go to the dashboard: the alert is gone, the rows say which model and whose profile, and
      the match form has the CV filled in.
- [ ] **The lists:** after a few matches, http://127.0.0.1:8765/match shows them under
      Shortlist, All and the other lists. Show more loads more.
- [ ] **The free match:** paste a job post's text, with evaluating set to no key on the key page.
      A match is saved and scored by the free reader.
- [ ] **The AI match:** pick a key for evaluating and paste another post's text. The result shows the
      requirements as the model read them, the score and, with the model in place, the Laya line.
- [ ] **Fetching a job from a link:** paste a link to a public job post (Jobindex or LinkedIn,
      say). The post is read from the page and matched. A link to this PC or the home network is
      refused.
- [ ] **Writing:** on a saved match, write the application, then the tailored résumé. Each one
      appears in its box. Write one again with notes, and the earlier version is listed under it.
- [ ] **The app's own model:** on /llama_setup, download llama.cpp and the recommended model,
      then Use this model; the API key page lists the model and how it fits. Save "This app's own model" with the model blank,
      press Check key (the first check loads the model), then run an AI match on it.
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
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace --release

The parity tests compare the matching with a fixed snapshot of expected answers
(`app/tests/fixtures/parity.json`). When a change is meant to move an answer, update the
snapshot in the same commit.

`app/README.md` has more on the tests.
