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

The taxonomy index (`taxonomy/data/index.json`) is built into the program.

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
CVs, and all 28 post x CV matches (criteria, verdicts, score, Laya summary).

The fixtures come from the PHP code itself. After changing the PHP side, write them again and
let the Rust tests show what the port has to follow:

    php desktop/recruiter/tests/fixtures/generate.php

`tests/import.rs` imports from a real MariaDB with `db/schema.sql` loaded. It empties that
database's cv_match tables, so point it at a scratch database only:

    RECRUITER_TEST_MYSQL=mysql://test:test@127.0.0.1:3306/recruiter_test cargo test --test import -- --ignored --test-threads=1

CI (`.github/workflows/recruiter-desktop.yml`) runs that against a MariaDB service, and also runs the generator and fails when the
committed fixtures no longer match what PHP answers.
