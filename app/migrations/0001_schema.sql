-- cv_match's tables, as db/schema.sql has them in MariaDB, for the one local user, without the
-- cv_match prefix (cv_matches is matches, cv_match_id is match_id, cv_match_items is
-- match_items); the rest of the names are MariaDB's. Ids are copied over by the import, so every
-- link between rows stays as it was. Timestamps are unix seconds. The written documents are the
-- one table MariaDB doesn't have (documents).
--
-- While the app is new, this file is changed in place rather than followed by migrations: a
-- database made from an older version of it is refused at start (db.rs), and made again with
-- `recruiter import`.

-- The local user. Only id is kept: there is no login on the desktop, and trongate_user_id
-- stays the key that matches and user_llm_keys hang on.
CREATE TABLE trongate_users (
  id INTEGER PRIMARY KEY
);

-- The candidate behind the local user, without the login columns (password, active,
-- num_logins, last_login).
CREATE TABLE candidates (
  id INTEGER PRIMARY KEY,
  trongate_user_id INTEGER NOT NULL UNIQUE REFERENCES trongate_users (id),
  name TEXT NOT NULL,
  email TEXT NOT NULL UNIQUE,
  phone TEXT DEFAULT NULL,
  postal_code TEXT DEFAULT NULL,
  open_to_work INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- A saved match: the job post and the CV it was matched with, and its score.
CREATE TABLE matches (
  id INTEGER PRIMARY KEY,
  trongate_user_id INTEGER DEFAULT NULL REFERENCES trongate_users (id) ON DELETE CASCADE,
  job_title TEXT NOT NULL,
  company TEXT NOT NULL DEFAULT '',
  job_url TEXT DEFAULT NULL,
  job_text TEXT NOT NULL,
  cv_name TEXT NOT NULL DEFAULT '',
  cv_text TEXT NOT NULL,
  score REAL NOT NULL,
  points INTEGER NOT NULL,
  max_points INTEGER NOT NULL,
  tag TEXT NOT NULL,
  rejected_at INTEGER DEFAULT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX matches_user_created ON matches (trongate_user_id, created_at);

-- One row per scored criterion item: the "why" behind a matches score.
CREATE TABLE match_items (
  id INTEGER PRIMARY KEY,
  match_id INTEGER NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
  criterion TEXT NOT NULL,
  text TEXT NOT NULL,
  verdict TEXT NOT NULL,
  decided_by TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  evidence TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX match_items_match_id ON match_items (match_id, sort_order);

-- A saved match's state in an outside service (Jobnet's joblog).
CREATE TABLE match_integrations (
  id INTEGER PRIMARY KEY,
  match_id INTEGER NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
  integration TEXT NOT NULL,
  status TEXT NOT NULL,
  external_id TEXT DEFAULT NULL,
  applied_at INTEGER DEFAULT NULL,
  logged_at INTEGER DEFAULT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (match_id, integration)
);

-- Why a match was marked Not interested, one row per reason.
CREATE TABLE match_rejections (
  id INTEGER PRIMARY KEY,
  match_id INTEGER NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
  reason TEXT NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  UNIQUE (match_id, reason)
);
CREATE INDEX match_rejections_reason ON match_rejections (reason);

-- A document written for a saved match, one row per version: kind application (the job
-- application) or resume (the tailored résumé). The newest of a kind is the one the page shows.
-- MariaDB keeps only the last one, in matches.application_text and resume_text.
-- written_by: the provider and model that wrote it ("openai/gpt-5.4-mini"), '' when it came
-- from the PHP app. notes: the notes the user gave for that writing, '' when none.
CREATE TABLE documents (
  id INTEGER PRIMARY KEY,
  match_id INTEGER NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  written_by TEXT NOT NULL DEFAULT '',
  notes TEXT NOT NULL DEFAULT '',
  written_at INTEGER NOT NULL
);
CREATE INDEX documents_match_kind ON documents (match_id, kind, written_at);

-- A tailored résumé's head, one per résumé version (documents of kind resume).
CREATE TABLE match_resumes (
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
CREATE TABLE match_resume_entries (
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
CREATE INDEX match_resume_entries_document_id ON match_resume_entries (document_id, section, sort_order);

-- A tailored résumé's bullets, skills and languages; entry_id is set for an entry's bullets.
CREATE TABLE match_resume_lines (
  id INTEGER PRIMARY KEY,
  document_id INTEGER NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
  entry_id INTEGER DEFAULT NULL REFERENCES match_resume_entries (id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX match_resume_lines_document_id ON match_resume_lines (document_id, kind, sort_order);
CREATE INDEX match_resume_lines_entry_id ON match_resume_lines (entry_id, sort_order);

-- The candidate's profile, which the writing uses.
CREATE TABLE candidate_profiles (
  candidate_id INTEGER PRIMARY KEY REFERENCES candidates (id) ON DELETE CASCADE,
  title TEXT NOT NULL DEFAULT '',
  location TEXT NOT NULL DEFAULT '',
  about TEXT NOT NULL,
  filled_from_cv_name TEXT NOT NULL DEFAULT '',
  filled_from_cv_at INTEGER DEFAULT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- The profile's roles, projects and schools; started_on/ended_on are YYYY-MM-DD.
CREATE TABLE candidate_profile_entries (
  id INTEGER PRIMARY KEY,
  candidate_id INTEGER NOT NULL REFERENCES candidates (id) ON DELETE CASCADE,
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
CREATE INDEX candidate_profile_entries_candidate_section ON candidate_profile_entries (candidate_id, section, sort_order);

-- The profile's bullets, skills and languages; entry_id is set for an entry's bullets.
CREATE TABLE candidate_profile_lines (
  id INTEGER PRIMARY KEY,
  candidate_id INTEGER NOT NULL REFERENCES candidates (id) ON DELETE CASCADE,
  entry_id INTEGER DEFAULT NULL REFERENCES candidate_profile_entries (id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX candidate_profile_lines_candidate_kind ON candidate_profile_lines (candidate_id, kind, sort_order);
CREATE INDEX candidate_profile_lines_entry_id ON candidate_profile_lines (entry_id, sort_order);

-- The candidate's profile links (LinkedIn, GitHub, a portfolio), in the order they put them.
CREATE TABLE candidate_profile_links (
  id INTEGER PRIMARY KEY,
  candidate_id INTEGER NOT NULL REFERENCES candidates (id) ON DELETE CASCADE,
  label TEXT NOT NULL,
  url TEXT NOT NULL,
  position INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (candidate_id, label)
);

-- The CV the profile was filled from.
CREATE TABLE candidate_resumes (
  id INTEGER PRIMARY KEY,
  candidate_id INTEGER NOT NULL UNIQUE REFERENCES candidates (id) ON DELETE CASCADE,
  version INTEGER NOT NULL DEFAULT 1,
  cv_name TEXT NOT NULL DEFAULT '',
  cv_text TEXT NOT NULL,
  current_title TEXT DEFAULT NULL,
  postal_code TEXT DEFAULT NULL,
  experience_years INTEGER DEFAULT NULL,
  language TEXT NOT NULL DEFAULT 'da',
  extractor TEXT NOT NULL DEFAULT 'manual',
  confirmed_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- The user's LLM: the API key itself lives in the system's credential store (Windows
-- Credential Manager), not here. base_url is set for a local model (Ollama, LM Studio).
-- use_for_*: the purposes the key is switched on for (MariaDB's uses SET), all by default.
CREATE TABLE user_llm_keys (
  id INTEGER PRIMARY KEY,
  trongate_user_id INTEGER NOT NULL UNIQUE REFERENCES trongate_users (id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  model TEXT NOT NULL DEFAULT '',
  base_url TEXT NOT NULL DEFAULT '',
  key_hint TEXT NOT NULL DEFAULT '',
  use_for_resume INTEGER NOT NULL DEFAULT 1,
  use_for_application INTEGER NOT NULL DEFAULT 1,
  use_for_job_evaluation INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
