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

The taxonomy index (`taxonomy/data/index.json`) is built into the program.

## Tests

    cargo test --release

`tests/parity.rs` runs every case in `tests/fixtures/parity.json` through the Rust code and
compares the answer with PHP's: 3,092 taxonomy lookups (the skill names of the 200 labelled ads
in `eval/data/gold.jsonl`, plus typos and edge cases), the free reading of synthetic posts and
CVs, and all 28 post x CV matches (criteria, verdicts, score, Laya summary).

The fixtures come from the PHP code itself. After changing the PHP side, write them again and
let the Rust tests show what the port has to follow:

    php desktop/recruiter/tests/fixtures/generate.php

CI (`.github/workflows/recruiter-desktop.yml`) also runs the generator and fails when the
committed fixtures no longer match what PHP answers.
