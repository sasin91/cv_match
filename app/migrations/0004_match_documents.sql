-- Every text a match is made of or writes is a document: the CV it was matched with (kind cv),
-- and each version of its application and tailored résumé. match_documents says which match
-- uses which, so a CV is kept once however many matches are made with it: the job scan matches
-- every post it finds with the profile, which made hundreds of matches each holding the same
-- CV text. A CV is its name and text; uploading it again with either changed makes a new
-- document, so a match always shows the CV it was scored with. matches keeps job_text: a post
-- is matched once, so there is nothing to share.
--
-- documents is made again without match_id (SQLite can't drop a NOT NULL column that is a
-- foreign key). db::migrate runs this with foreign keys off, as SQLite's own way of remaking a
-- table asks, so dropping the old table doesn't delete what hangs on it (match_resumes and its
-- entries and lines, keyed on document_id, which keep their ids), and checks every foreign key
-- before committing.
CREATE TABLE documents_new (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '',
  text TEXT NOT NULL,
  written_by TEXT NOT NULL DEFAULT '',
  notes TEXT NOT NULL DEFAULT '',
  written_at INTEGER NOT NULL
);

CREATE TABLE match_documents (
  match_id INTEGER NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
  document_id INTEGER NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
  PRIMARY KEY (match_id, document_id)
);
CREATE INDEX match_documents_document ON match_documents (document_id);

-- The applications and résumés, with their ids, each on its match.
INSERT INTO documents_new (id, kind, text, written_by, notes, written_at)
SELECT id, kind, text, written_by, notes, written_at FROM documents;
INSERT INTO match_documents (match_id, document_id)
SELECT match_id, id FROM documents;

-- The CVs the saved matches were made with, each once, oldest first, on the matches made with
-- them.
INSERT INTO documents_new (kind, name, text, written_at)
SELECT 'cv', cv_name, cv_text, MIN(created_at)
FROM matches
GROUP BY cv_name, cv_text
ORDER BY MIN(created_at);
INSERT INTO match_documents (match_id, document_id)
SELECT m.id, d.id
FROM matches m
JOIN documents_new d ON d.kind = 'cv' AND d.name = m.cv_name AND d.text = m.cv_text;

DROP TABLE documents;
ALTER TABLE documents_new RENAME TO documents;
CREATE INDEX documents_kind ON documents (kind, written_at);

ALTER TABLE matches DROP COLUMN cv_name;
ALTER TABLE matches DROP COLUMN cv_text;

-- A document no match uses any more goes with the last match that used it, as an application
-- went with its match before.
CREATE TRIGGER match_documents_unused AFTER DELETE ON match_documents
WHEN NOT EXISTS (SELECT 1 FROM match_documents WHERE document_id = OLD.document_id)
BEGIN
  DELETE FROM documents WHERE id = OLD.document_id;
END;
