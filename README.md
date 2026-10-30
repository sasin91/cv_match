# cv_match

Recruiter's CV match as a desktop app: one program for Windows (or any desktop) that keeps your
saved matches in SQLite, serves the match page on http://127.0.0.1:8765 in your own browser, and
works with Jobnet's joblog through its browser extension. One local user, no sign-in.

It is ported from the PHP app [sasin91/recruiter](https://github.com/sasin91/recruiter), file by
file and with the same names, and tested against what the PHP code answers.

| Folder | What |
| --- | --- |
| `app/` | the app (`recruiter.exe`): the matching, the pages, the database, the import from the PHP app's MariaDB. See its README. |
| `laya-server/` | Laya's `/v1/systemone` in Rust on ONNX Runtime, to be built into the app |
| `assets/` | what the app builds in: from recruiter cv_match's CSS and JS, Trongate MX, the taxonomy index and the Danish postcodes; pdf.js 4.10.38 (`match/pdfjs`) and DejaVu Sans 2.37 (`fonts`) |
| `extension/jobnet-joblog/` | the Jobnet extension the app ships and you load once in Edge or Chrome |

    cd app
    cargo run --release
