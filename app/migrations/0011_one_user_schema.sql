-- Tables shaped for the one local user, now that there is no user table (0010_one_local_user).
-- The web app's candidates row and its candidate_profiles header become one profile row, the
-- profile's lists no longer say whose they are, and the language model tables lose user_.
-- Every row is kept. db::migrate runs this with foreign keys off and checks them all before it
-- commits.

-- The profile: who the user is and what the CV says about them, in its one row (id is always
-- 1). filled_from_cv_*: the CV it was last filled from ('' and NULL when written by hand or
-- imported). candidates' postal_code and open_to_work aren't kept: nothing reads them.
CREATE TABLE profile (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  name TEXT NOT NULL,
  email TEXT NOT NULL DEFAULT '',
  phone TEXT DEFAULT NULL,
  title TEXT NOT NULL DEFAULT '',
  location TEXT NOT NULL DEFAULT '',
  about TEXT NOT NULL DEFAULT '',
  filled_from_cv_name TEXT NOT NULL DEFAULT '',
  filled_from_cv_at INTEGER DEFAULT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
-- There is at most one candidate (its trongate_user_id was UNIQUE).
INSERT INTO profile
  (id, name, email, phone, title, location, about, filled_from_cv_name, filled_from_cv_at,
   created_at, updated_at)
SELECT 1, c.name, c.email, c.phone, COALESCE(p.title, ''), COALESCE(p.location, ''),
       COALESCE(p.about, ''), COALESCE(p.filled_from_cv_name, ''), p.filled_from_cv_at,
       c.created_at, MAX(c.updated_at, COALESCE(p.updated_at, c.updated_at))
FROM candidates c
LEFT JOIN candidate_profiles p ON p.candidate_id = c.id
ORDER BY c.id
LIMIT 1;

-- The profile's experience, projects and education, as the CV lists them.
CREATE TABLE profile_entries (
  id INTEGER PRIMARY KEY,
  section TEXT NOT NULL,
  title TEXT NOT NULL,
  organisation TEXT NOT NULL DEFAULT '',
  location TEXT NOT NULL DEFAULT '',
  url TEXT NOT NULL DEFAULT '',
  started_on TEXT DEFAULT NULL,
  ended_on TEXT DEFAULT NULL,
  summary TEXT NOT NULL DEFAULT '',
  note TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
INSERT INTO profile_entries
  (id, section, title, organisation, location, url, started_on, ended_on, summary, note,
   sort_order, created_at, updated_at)
SELECT id, section, title, organisation, location, url, started_on, ended_on, summary, note,
       sort_order, created_at, updated_at
FROM candidate_profile_entries;
CREATE INDEX profile_entries_section ON profile_entries (section, sort_order);

-- The profile's single lines: an entry's bullets (entry_id set), and the skills, languages and
-- certificates (entry_id NULL).
CREATE TABLE profile_lines (
  id INTEGER PRIMARY KEY,
  entry_id INTEGER DEFAULT NULL REFERENCES profile_entries (id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0
);
INSERT INTO profile_lines (id, entry_id, kind, text, sort_order)
SELECT id, entry_id, kind, text, sort_order FROM candidate_profile_lines;
CREATE INDEX profile_lines_kind ON profile_lines (kind, sort_order);
CREATE INDEX profile_lines_entry_id ON profile_lines (entry_id, sort_order);

-- The profile's links (LinkedIn, GitHub, ...), one per label.
CREATE TABLE profile_links (
  id INTEGER PRIMARY KEY,
  label TEXT NOT NULL UNIQUE,
  url TEXT NOT NULL,
  position INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
INSERT INTO profile_links (id, label, url, position, created_at, updated_at)
SELECT id, label, url, position, created_at, updated_at FROM candidate_profile_links;

DROP TABLE candidate_profile_lines;
DROP TABLE candidate_profile_entries;
DROP TABLE candidate_profile_links;
DROP TABLE candidate_profiles;
-- Never read or written by the app.
DROP TABLE candidate_resumes;
DROP TABLE candidates;

-- The saved language model keys, one per provider (the keys themselves are in Credential
-- Manager), and the model each task runs on.
ALTER TABLE user_llm_keys RENAME TO llm_keys;
ALTER TABLE user_llm_use_models RENAME TO llm_task_models;
ALTER TABLE llm_task_models RENAME COLUMN use TO task;
