-- The taxonomy in the database: the shipped index (assets/taxonomy/index.json, built into the
-- program) written into these tables at start whenever it changes, and the labels the app learns
-- from matches beside it. taxonomy_store builds the one Taxonomy every lookup uses from the
-- terms and their active labels.
--
-- A term's id is its id in the shipped index; a term the app makes itself starts at 1,000,000
-- (taxonomy_store::FIRST_LEARNED_TERM_ID). definition is the shipped index's, for its words.
CREATE TABLE taxonomy_terms (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('skill', 'title', 'role', 'skill_category')),
  parent_id INTEGER DEFAULT NULL REFERENCES taxonomy_terms (id) DEFERRABLE INITIALLY DEFERRED,
  definition TEXT DEFAULT NULL,
  source TEXT NOT NULL CHECK (source IN ('shipped', 'learned'))
);
CREATE INDEX taxonomy_terms_kind ON taxonomy_terms (kind);
CREATE INDEX taxonomy_terms_parent ON taxonomy_terms (parent_id);

-- A term's names. preferred marks its preferred label in a language (shipped only). A learned
-- label starts as a candidate, which matching doesn't use, and turns active at
-- taxonomy_store::SIGHTINGS_TO_ACTIVATE sightings or when it is approved by hand; a rejected one
-- is kept so it isn't learned again. A shipped label that is edited or rejected becomes learned,
-- so writing the shipped index again doesn't bring it back.
CREATE TABLE taxonomy_labels (
  id INTEGER PRIMARY KEY,
  term_id INTEGER NOT NULL REFERENCES taxonomy_terms (id) ON DELETE CASCADE,
  text TEXT NOT NULL,
  lang TEXT NOT NULL CHECK (lang IN ('da', 'en')),
  preferred INTEGER NOT NULL DEFAULT 0,
  source TEXT NOT NULL CHECK (source IN ('shipped', 'learned')),
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'candidate', 'rejected')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (term_id, text, lang)
);
CREATE INDEX taxonomy_labels_term ON taxonomy_labels (term_id);
CREATE INDEX taxonomy_labels_status ON taxonomy_labels (status, source);
CREATE INDEX taxonomy_labels_text ON taxonomy_labels (text COLLATE NOCASE);

-- Each match that taught a learned label: from the model's read of the post (read) or from the
-- judge's met verdict (judge), and the provider/model that answered. The first sighting is the
-- oldest seen_at.
CREATE TABLE taxonomy_label_sightings (
  id INTEGER PRIMARY KEY,
  label_id INTEGER NOT NULL REFERENCES taxonomy_labels (id) ON DELETE CASCADE,
  match_id INTEGER NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
  learned_from TEXT NOT NULL CHECK (learned_from IN ('read', 'judge')),
  model TEXT NOT NULL DEFAULT '',
  seen_at INTEGER NOT NULL,
  UNIQUE (label_id, match_id)
);
CREATE INDEX taxonomy_label_sightings_match ON taxonomy_label_sightings (match_id);
CREATE INDEX taxonomy_label_sightings_label ON taxonomy_label_sightings (label_id, seen_at);

-- The shipped index last written to the tables: one row, the sha256 of its JSON.
CREATE TABLE taxonomy_shipped (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  index_sha256 TEXT NOT NULL,
  written_at INTEGER NOT NULL
);
