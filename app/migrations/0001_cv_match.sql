-- cv_match's tables, as db/schema.sql has them in MariaDB, for the one local user. Same table
-- and column names, so the PHP app's names keep meaning the same; ids are copied over by the
-- import, so every link between rows stays as it was. Timestamps are unix seconds.

-- The local user. Only id is kept: there is no login on the desktop, and trongate_user_id
-- stays the key that cv_matches and user_llm_keys hang on.
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
CREATE TABLE cv_matches (
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
  application_text TEXT DEFAULT NULL,
  application_written_at INTEGER DEFAULT NULL,
  resume_text TEXT DEFAULT NULL,
  resume_written_at INTEGER DEFAULT NULL,
  rejected_at INTEGER DEFAULT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX cv_matches_user_created ON cv_matches (trongate_user_id, created_at);

-- One row per scored criterion item: the "why" behind a cv_matches score.
CREATE TABLE cv_match_items (
  id INTEGER PRIMARY KEY,
  cv_match_id INTEGER NOT NULL REFERENCES cv_matches (id) ON DELETE CASCADE,
  criterion TEXT NOT NULL,
  text TEXT NOT NULL,
  verdict TEXT NOT NULL,
  decided_by TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  evidence TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX cv_match_items_cv_match_id ON cv_match_items (cv_match_id, sort_order);

-- A saved match's state in an outside service (Jobnet's joblog).
CREATE TABLE cv_match_integrations (
  id INTEGER PRIMARY KEY,
  cv_match_id INTEGER NOT NULL REFERENCES cv_matches (id) ON DELETE CASCADE,
  integration TEXT NOT NULL,
  status TEXT NOT NULL,
  external_id TEXT DEFAULT NULL,
  applied_at INTEGER DEFAULT NULL,
  logged_at INTEGER DEFAULT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (cv_match_id, integration)
);

-- Why a match was marked Not interested, one row per reason.
CREATE TABLE cv_match_rejections (
  id INTEGER PRIMARY KEY,
  cv_match_id INTEGER NOT NULL REFERENCES cv_matches (id) ON DELETE CASCADE,
  reason TEXT NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  UNIQUE (cv_match_id, reason)
);
CREATE INDEX cv_match_rejections_reason ON cv_match_rejections (reason);

-- A tailored résumé's head, one per saved match.
CREATE TABLE cv_match_resumes (
  cv_match_id INTEGER PRIMARY KEY REFERENCES cv_matches (id) ON DELETE CASCADE,
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
  cv_match_id INTEGER NOT NULL REFERENCES cv_matches (id) ON DELETE CASCADE,
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
CREATE INDEX cv_match_resume_entries_cv_match_id ON cv_match_resume_entries (cv_match_id, section, sort_order);

-- A tailored résumé's bullets, skills and languages; entry_id is set for an entry's bullets.
CREATE TABLE cv_match_resume_lines (
  id INTEGER PRIMARY KEY,
  cv_match_id INTEGER NOT NULL REFERENCES cv_matches (id) ON DELETE CASCADE,
  entry_id INTEGER DEFAULT NULL REFERENCES cv_match_resume_entries (id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX cv_match_resume_lines_cv_match_id ON cv_match_resume_lines (cv_match_id, kind, sort_order);
CREATE INDEX cv_match_resume_lines_entry_id ON cv_match_resume_lines (entry_id, sort_order);

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
