-- The documents written for a saved match move out of cv_matches into their own table, one row
-- per version, so writing one again keeps the one before. The tailored résumé's fields hang on
-- the résumé version they were written with, not on the match.

-- A document written for a saved match, one row per version: kind application (the job
-- application) or resume (the tailored résumé). The newest of a kind is the one the page shows.
-- written_by: the provider and model that wrote it ("openai/gpt-5.4-mini"), '' when it came
-- from the PHP app. notes: the notes the user gave for that writing, '' when none.
CREATE TABLE documents (
  id INTEGER PRIMARY KEY,
  cv_match_id INTEGER NOT NULL REFERENCES cv_matches (id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  written_by TEXT NOT NULL DEFAULT '',
  notes TEXT NOT NULL DEFAULT '',
  written_at INTEGER NOT NULL
);
CREATE INDEX documents_cv_match_kind ON documents (cv_match_id, kind, written_at);

-- What was written so far, as the first version of each. A résumé with fields but no text
-- (never written so by the PHP app, but the fields mustn't be lost) gets an empty text.
INSERT INTO documents (cv_match_id, kind, text, written_at)
SELECT id, 'application', application_text, COALESCE(application_written_at, created_at)
FROM cv_matches
WHERE application_text IS NOT NULL;

INSERT INTO documents (cv_match_id, kind, text, written_at)
SELECT m.id, 'resume', COALESCE(m.resume_text, ''), COALESCE(m.resume_written_at, m.created_at)
FROM cv_matches m
WHERE m.resume_text IS NOT NULL
   OR EXISTS (SELECT 1 FROM cv_match_resumes r WHERE r.cv_match_id = m.id);

ALTER TABLE cv_matches DROP COLUMN application_text;
ALTER TABLE cv_matches DROP COLUMN application_written_at;
ALTER TABLE cv_matches DROP COLUMN resume_text;
ALTER TABLE cv_matches DROP COLUMN resume_written_at;

-- The résumé tables are made again keyed on document_id (SQLite can't change a key in place):
-- the old ones step aside, the rows are copied with their ids, then the old ones go. Their
-- indexes are made last, once the old ones (with the same names) are gone.
ALTER TABLE cv_match_resume_lines RENAME TO cv_match_resume_lines_before;
ALTER TABLE cv_match_resume_entries RENAME TO cv_match_resume_entries_before;
ALTER TABLE cv_match_resumes RENAME TO cv_match_resumes_before;

-- A tailored résumé's head, one per résumé version (documents of kind resume).
CREATE TABLE cv_match_resumes (
  document_id INTEGER PRIMARY KEY REFERENCES documents (id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  location TEXT NOT NULL DEFAULT '',
  phone TEXT NOT NULL DEFAULT '',
  email TEXT NOT NULL DEFAULT '',
  links TEXT NOT NULL DEFAULT '',
  intro TEXT NOT NULL,
  education_note TEXT NOT NULL DEFAULT '',
  experience_heading TEXT NOT NULL DEFAULT 'Experience',
  skills_heading TEXT NOT NULL DEFAULT 'Skills',
  education_heading TEXT NOT NULL DEFAULT 'Education',
  languages_heading TEXT NOT NULL DEFAULT 'Languages',
  projects_heading TEXT NOT NULL DEFAULT 'Projects'
);

-- A tailored résumé's roles, projects and schools, in order.
CREATE TABLE cv_match_resume_entries (
  id INTEGER PRIMARY KEY,
  document_id INTEGER NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
  section TEXT NOT NULL,
  title TEXT NOT NULL,
  organisation TEXT NOT NULL DEFAULT '',
  location TEXT NOT NULL DEFAULT '',
  starts TEXT NOT NULL DEFAULT '',
  ends TEXT NOT NULL DEFAULT '',
  summary TEXT NOT NULL DEFAULT '',
  note TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL DEFAULT 0
);

-- A tailored résumé's bullets, skills and languages; entry_id is set for an entry's bullets.
CREATE TABLE cv_match_resume_lines (
  id INTEGER PRIMARY KEY,
  document_id INTEGER NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
  entry_id INTEGER DEFAULT NULL REFERENCES cv_match_resume_entries (id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0
);

-- Each match has at most one résumé document so far, the one made just above.
INSERT INTO cv_match_resumes
SELECT d.id, r.name, r.title, r.location, r.phone, r.email, r.links, r.intro, r.education_note,
       r.experience_heading, r.skills_heading, r.education_heading, r.languages_heading,
       r.projects_heading
FROM cv_match_resumes_before r
JOIN documents d ON d.cv_match_id = r.cv_match_id AND d.kind = 'resume';

INSERT INTO cv_match_resume_entries
SELECT e.id, d.id, e.section, e.title, e.organisation, e.location, e.starts, e.ends, e.summary,
       e.note, e.sort_order
FROM cv_match_resume_entries_before e
JOIN documents d ON d.cv_match_id = e.cv_match_id AND d.kind = 'resume';

INSERT INTO cv_match_resume_lines
SELECT l.id, d.id, l.entry_id, l.kind, l.text, l.sort_order
FROM cv_match_resume_lines_before l
JOIN documents d ON d.cv_match_id = l.cv_match_id AND d.kind = 'resume';

DROP TABLE cv_match_resume_lines_before;
DROP TABLE cv_match_resume_entries_before;
DROP TABLE cv_match_resumes_before;

CREATE INDEX cv_match_resume_entries_document_id ON cv_match_resume_entries (document_id, section, sort_order);
CREATE INDEX cv_match_resume_lines_document_id ON cv_match_resume_lines (document_id, kind, sort_order);
CREATE INDEX cv_match_resume_lines_entry_id ON cv_match_resume_lines (entry_id, sort_order);
