# recruiter (desktop)

Recruiter's match as a standalone desktop app in Rust: one program with SQLite, Laya built in
and one local user, serving the match page in your browser. Its matching core is checked
against a fixed snapshot of expected answers (tests/parity.rs).

## What is where

| Rust | What |
| --- | --- |
| `src/taxonomy.rs` | the lexical taxonomy lookup (match ranks without an embedding model) |
| `src/matches/free_reader.rs` | the free reading of a post and a CV, with the words it ignores and the soft skills |
| `src/matches/matcher.rs` | matching a post against a CV, and the legacy ranking |
| `src/matches/match_score.rs` | a match score as the pages show it |
| `src/db.rs`, `migrations/`, `queries/` | the SQLite tables and the longer queries |
| `src/server.rs`, `src/matches/pages.rs` | the server, the match page and its endpoints |
| `src/matches/match_model.rs` | saved matches in the database |
| `src/matches/match_result.rs`, `saved_match_row.rs`, `batch_row.rs`, `verdict_row.rs`, `location_notice.rs`, `apply_notice.rs`, `reject_button.rs`, `reject_reason.rs`, `match_documents.rs`, `document_box.rs`, `score_cursor.rs` | the view models the templates print |
| `src/matches/apply_instructions.rs` | how a post asks to be applied to |
| `templates/` | the pages and fragments, which only print |
| `src/profile.rs` | the profile as text |
| `src/matches/joblog.rs`, `joblog_panel.rs` | the Jobnet joblog entry (with `assets/danish_postcodes.json`) and its panel |
| `src/extension.rs` | ships `extension/jobnet-joblog` and writes it out for the browser |
| `src/matches/pdf_writer.rs`, `tailored_resume.rs` | documents as PDFs, and the tailored résumé's fields |
| `src/matches/match_prompts.rs` | extract_job, extract_cv and the judging |
| `src/matches/ai_match.rs`, `document_writer.rs`, `llm_key.rs` | the AI match, the writing, and which key a use runs on |
| `src/matches/page_reader.rs` | fetch_job: a post read from a pasted link |
| `src/matches/application_delivery.rs`, `cv_links.rs`, `resume_comparison.rs` | the Apply dialog and the résumé's comparison |
| `src/laya/laya_client.rs` | Laya, on `laya-server`'s model in this process |
| `src/pdf/` | lays the page out on A4 as dompdf 3.1 did and writes the PDF |

What the app builds in is in `../assets` (the page's CSS and JS, Trongate MX, the taxonomy
index and the Danish postcodes); the Jobnet extension lives in `../extension/jobnet-joblog`.

## Running it

    cargo run --release

serves the app on http://127.0.0.1:8765/match and opens it in your default browser
(`--port`, `--database file`, `--laya folder`, `--no-browser`, and `--tailscale-name` for your phone:
see [../docs/phone.md](../docs/phone.md)). It is the match page,
with its CSS and JS (`assets/match/css`, `js`, and Trongate MX from `assets/js`, built into the exe), for
one local user who is always signed in:

- the saved matches by list (Shortlist, All, Applied not on Jobnet yet, On Jobnet, Rejected),
  best first, with Show more and the 30-second refresh;
- a saved match's result, with its notices (location, how to apply) and its application and
  résumé as written;
- Not interested and Bring back;
- the free keyword match of new posts against your profile or a pasted CV;
- with your own key (below), the AI match: the post and CV read by the model (extract_job,
  extract_cv), what the taxonomy can't decide judged by it, scored and saved; a post read from a
  pasted link (fetch_job), public web pages only, never this computer or the home network;
- the job application and the tailored résumé written for a saved match, each writing kept as
  a new version: the box shows the newest, with the earlier ones listed under it (when, by which
  model, with which notes) to copy or download as a PDF; the résumé's own match against the post
  next to the CV's;
- the Apply dialog: by email (recipient, subject, body, mailto link) or on the employer's site,
  with the PDFs and your profile links;
- a match's Jobnet joblog panel, and what the Jobnet extension asks for (the history, a joblog
  entry, marking a match as on Jobnet, the PDFs it attaches);
- the application, the tailored résumé, the CV and the job post as PDFs (`POST match/pdf`);
- reading an uploaded PDF CV's text in the app (`POST match/pdf_text`, `src/pdf_text`, from
  sappkit's PDF reader), so it works offline and a desktop window can read it too;
- the API key page (http://127.0.0.1:8765/account), below;
- Laya's reading of the free match and the AI match, when its model is there (below).

## Laya

Laya reads a match's requirement verdicts (the Laya summary) and answers how likely the CV meets
every requirement and how good a fit it is, 0 to 4; the result shows it as one line under the
score. The model is `laya-server`'s, run inside the app on ONNX Runtime (linked into the exe), so
no Laya service or Python is needed. It needs the exported English checkpoint in one folder,
`%APPDATA%\Recruiter\laya` (next to the database, or `--laya folder`):

    laya\rl_agent_config.json
    laya\tokenizer\            (tokenizer.json, tokenizer_config.json, ...)
    laya\laya.onnx
    laya\laya.onnx.data         (1.7 GB)

that is, the checkpoint folder and the two files `export.py` writes (see
`../laya-server/README.md`, "One-time: export the model"). Without `laya.onnx` there, the app
says so at start and the matches go on without a Laya line. The model loads in the background at start (a few seconds, about 1.5 GB of memory); a match made
before then waits for it. When it can't load or answer, the result says why under the score and
the log has it too; the score doesn't depend on it. There is no per-visitor limit: the model
answers only you.

## PDFs

`pdf_writer` reads a document and builds a small page (`pdf::page`, which prints as HTML; the
parity tests compare it). `pdf::layout` places it on A4 the way dompdf 3.1 does: Helvetica, 16 mm side and
18 mm top and bottom margins, the page's styles, dompdf's line boxes, margin collapsing and page
breaks (div.keep, two-line orphans). Checked against dompdf's own PDFs of 120 generated letters,
résumés, CVs and job posts, every line lands on the same page within 0.1 pt. Text with a letter
Helvetica lacks is set in DejaVu Sans, with only the letters used embedded
(`assets/fonts`, Bitstream Vera licence).

The server only answers requests whose Host is its own address, and only takes changes (POST)
from its own pages and the Jobnet extension (their Origin), so other sites open in the browser
can't use it.

## The API key and the language models

The AI features run on your own keys, saved on the API key page (`account`): an OpenAI key, an Anthropic key, or a local OpenAI-compatible server
such as Ollama (`http://localhost:11434/v1`) or LM Studio (`http://localhost:1234/v1`), which
needs a model name and usually no key. One key is kept per provider. Each goes in Windows
Credential Manager (service `Recruiter`, user `user_llm_keys:1:{provider}@` and the database
file's full path, so each provider and each database has its own key), never in the database;
`user_llm_keys` keeps a row per provider with the model, address, the key's last 4 characters
and the uses it is picked for (each use on one key at most). A key saved before keys were kept
per provider (`user_llm_keys:1@…`) moves to its provider's credential the first time it is read. "Check key" sends one
tiny request and says whether it was answered.

`src/llm` is modules/llm: `Llm::use_key` picks the adapter, and `structured()` asks for one JSON
answer that follows a schema. `openai` speaks Chat Completions (and the Responses API with web
search), also for a local server at its `base_url`; `anthropic` speaks the Messages API with
structured output, effort and server-side refusal fallbacks. With no model named, a key runs on
gpt-5.4-mini (OpenAI) or claude-sonnet-5-5 (Anthropic). A call waits up to 180 seconds; errors
come back in the provider's own words for the page, and the log gets the status, the rate-limit
headers and the body, with any key in it cut to its prefix.

The match page runs each AI step on the key picked for that use: evaluating job
posts (extract_job, extract_cv, the judging in match), writing applications, tailoring résumés.
With no key picked for evaluating, the free reader reads the post and CV instead, so a match is still
saved and can have its application and résumé written. Without a key for a use, the page says
why (none saved, none picked for the use, or gone from Credential Manager) and links to the API key page.

## The Jobnet extension

The app carries `extension/jobnet-joblog` and writes it to a `jobnet-joblog` folder next to the
database at every start (`%APPDATA%\Recruiter\jobnet-joblog`). Load that folder once in Edge or
Chrome (extensions page, Developer mode, "Load unpacked"); the app's page
http://127.0.0.1:8765/joblog_extension shows the folder and the steps. After an update, press
Reload on the extension. Its manifest `key` fixes its ID, the one `src/extension.rs` lets in.

## Database

The app keeps its data in one SQLite file, `%APPDATA%\Recruiter\recruiter.sqlite` on Windows.
Its tables are cv_match's MariaDB tables (`migrations/`, applied in order at start-up and
recorded in `schema_migrations`) without the `cv_match` prefix, since the whole app is the
match: `cv_matches` is `matches`, `cv_match_id` is `match_id`, `cv_match_items` is
`match_items`, and so on. Besides the names, there are four changes:

- `trongate_users` keeps only `id`, and `candidates` drops the login columns.
- `user_llm_keys.uses` (a SET) is three yes/no columns, `use_for_resume`,
  `use_for_application` and `use_for_job_evaluation`; `base_url` is set for a local model.
- The API key itself goes in Windows Credential Manager, so the table has no key column.
- The documents written for a match (its application and tailored résumé) are versions in
  `documents` instead of `matches.application_text` and `resume_text`, so writing one again
  keeps the one before; the newest is the one shown. Each version records the model that wrote
  it (`written_by`) and the notes it was written with. The résumé's fields (`match_resumes`,
  `match_resume_entries`, `match_resume_lines`) hang on the résumé version
  (`document_id`) instead of the match.

While the app is new, `migrations/0001_schema.sql` is changed in place instead of getting new
migrations. A database made from an older version of it is refused at start with a message
saying so: move `recruiter.sqlite` aside and start again.

### Queries

The app talks to its database through [sqlx](https://github.com/launchbadge/sqlx). Its queries
use the `query!` macros, which check the SQL, its parameters and the Rust types of its columns
at compile time. A query of more than a few lines is its own file in `queries/` (read with
`query_file!`); a short one is a raw string `r#"..."#` laid out like a heredoc where it is used.

The checks run against `.sqlx/`, the descriptions of every query, which is committed, so a
plain `cargo build` needs no database. After adding or changing a query, write them again
against a database made from `migrations/` (`cargo install sqlx-cli --no-default-features
--features sqlite` once):

    $env:DATABASE_URL = "sqlite:target/sqlx-dev.sqlite"   # PowerShell; export DATABASE_URL=... elsewhere
    sqlx database reset -y
    cargo sqlx prepare -- --all-targets

and commit `.sqlx/` with the change. CI fails when it doesn't match the queries.

## Tests

    cargo test --release

`tests/parity.rs` runs every case in `tests/fixtures/parity.json` through the Rust code and
compares the answer with the expected one: 3,092 taxonomy lookups (the skill names of the 200 labelled ads
in `eval/data/gold.jsonl`, plus typos and edge cases), the free reading of synthetic posts and
CVs, all 28 post x CV matches (criteria, verdicts, score, Laya summary) and 27 texts read for how
the post asks to be applied to, and the joblog: 343 deadlines, working hours, places and contact
people, 16 saved matches filled in as joblog entries and 10 days applied in Danish time; and
the 53 PDF pages and 7 download names Pdf_writer makes of letters, résumés, CVs and job posts;
and Laya's two questions, 11 of its answers as Laya_client reads them and the line Match_result
prints for each and for a failure. The model itself isn't in the repo, so the tests never run
it; `laya-server`'s own tests check it against Python's laya when its files are there.

The fixtures are a fixed snapshot of what the web app answered for these cases. When a change
to the matching is meant to change an answer, change the expected answer in `parity.json` in
the same commit, so the diff shows what moved.

`tests/server.rs` sends the page's requests to the server: the lists, a saved match, the free
match, the AI match and writing against a stand-in for a local model on 127.0.0.1, the Apply
dialog, the joblog, the extension's calls, the PDFs, the API key page, links that may not be
fetched, and requests from other sites. `tests/llm.rs` calls each adapter against a stand-in for its API on
127.0.0.1 and checks the request it sends and how it reads the answers and failures.

CI (`.github/workflows/app.yml`) runs fmt, clippy and the tests on Windows, and checks `.sqlx/` is what the queries are.
