# recruiter (desktop)

Recruiter's cv_match as a standalone desktop app in Rust: one program with SQLite, Laya built
in and one local user, serving the same pages, JS and CSS as the PHP app. It is ported from the
PHP modules file by file, keeping their names, and is tested against what the PHP code answers.

## What is here so far

| Rust | Ported from |
| --- | --- |
| `src/taxonomy.rs` | `taxonomy/php/Taxonomy.php`, the lexical lookup (cv_match ranks without the embedding model, so `Static_model` and `index.q8.bin` aren't needed) |
| `src/cv_match/free_reader.rs` | `modules/cv_match/Free_reader.php` (with `free_reader_ignore.php` and `free_reader_soft.php`) |
| `src/cv_match/cv_matcher.rs` | `modules/cv_match/Cv_matcher.php` |
| `src/cv_match/match_score.rs` | `modules/cv_match/Match_score.php` |
| `src/php.rs` | the PHP built-ins whose exact behaviour matters (`round`, `trim`) |
| `src/db.rs`, `migrations/` | `db/schema.sql`'s cv_match tables, for SQLite |
| `src/import.rs` | (new) the one-time copy from MariaDB |
| `src/server.rs`, `src/cv_match/pages.rs` | `Cv_match.php` (the page and its endpoints) |
| `src/cv_match/cv_match_model.rs` | `Cv_match_model.php` |
| `src/cv_match/match_result.rs`, `saved_match_row.rs`, `batch_row.rs`, `verdict_row.rs`, `location_notice.rs`, `apply_notice.rs`, `reject_button.rs`, `reject_reason.rs`, `match_documents.rs`, `document_box.rs`, `score_cursor.rs` | the view models of the same names |
| `src/cv_match/apply_instructions.rs` | `Apply_instructions.php` |
| `templates/cv_match/*.html` | `modules/cv_match/views/*.php` |
| `src/profile.rs` | the profile as text (`Profile_fields::text` and its queries) |

The taxonomy index (`taxonomy/data/index.json`) is built into the program.

## Running it

    cargo run --release

serves the app on http://127.0.0.1:8765/cv_match and opens it in your default browser
(`--port`, `--database file`, `--no-browser`). It is the PHP app's match page with the same CSS
and JS (`modules/cv_match/css`, `js`, and Trongate MX from `public/js`, built into the exe), for
one local user who is always signed in:

- the saved matches by list (Shortlist, All, Applied not on Jobnet yet, On Jobnet, Rejected),
  best first, with Show more and the 30-second refresh;
- a saved match's result, with its notices (location, how to apply) and its application and
  résumé as written;
- Not interested and Bring back;
- the free keyword match of new posts against your profile or a pasted CV.

What isn't ported yet (the AI match, fetching a post from a link, writing, PDFs, the Apply
dialog, Jobnet's joblog) answers with "... isn't in the desktop app yet.", which the page shows
as its error.

The server only answers requests whose Host is its own address, and only takes changes (POST)
from its own pages (their Origin), so other sites open in the browser can't use it.

## Database

The app keeps its data in one SQLite file, `%APPDATA%\Recruiter\recruiter.sqlite` on Windows.
Its tables are cv_match's MariaDB tables with the same names (`migrations/`, applied in order at
start-up and recorded in `schema_migrations`), with three changes for one local user:

- `trongate_users` keeps only `id`, and `candidates` drops the login columns.
- `user_llm_keys.uses` (a SET) is three yes/no columns, `use_for_resume`,
  `use_for_application` and `use_for_job_evaluation`; `base_url` is set for a local model.
- The API key itself goes in Windows Credential Manager, so the table has no key column.

To copy your data from the PHP app's MariaDB (Herd's copy of production, `recruiter_prod`):

    recruiter import --email you@example.com

`--from mysql://user:pass@host:3306/db` reads another database and `--database file` writes
another SQLite file. Only the candidate with that email is copied, with every id as it was, all
in one transaction. It refuses a database that already has your data. Your LLM key isn't
copied, because it is encrypted with the server's secret: add it again in the app.

## Tests

    cargo test --release

`tests/parity.rs` runs every case in `tests/fixtures/parity.json` through the Rust code and
compares the answer with PHP's: 3,092 taxonomy lookups (the skill names of the 200 labelled ads
in `eval/data/gold.jsonl`, plus typos and edge cases), the free reading of synthetic posts and
CVs, all 28 post x CV matches (criteria, verdicts, score, Laya summary) and 27 texts read for how
the post asks to be applied to.

The fixtures come from the PHP code itself. After changing the PHP side, write them again and
let the Rust tests show what the port has to follow:

    php desktop/recruiter/tests/fixtures/generate.php

`tests/server.rs` sends the page's requests to the server: the lists, a saved match, the free
match, what isn't ported yet, and requests from other sites.

`tests/import.rs` imports from a real MariaDB with `db/schema.sql` loaded. It empties that
database's cv_match tables, so point it at a scratch database only:

    RECRUITER_TEST_MYSQL=mysql://test:test@127.0.0.1:3306/recruiter_test cargo test --test import -- --ignored --test-threads=1

CI (`.github/workflows/recruiter-desktop.yml`) runs that against a MariaDB service, and also runs the generator and fails when the
committed fixtures no longer match what PHP answers.
