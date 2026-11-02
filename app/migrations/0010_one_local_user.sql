-- The desktop app has one local user and no login, so trongate_users goes, and with it every
-- trongate_user_id: matches, candidates, user_llm_keys and user_llm_use_models now belong to
-- the one user the app has. SQLite can't drop a column that a foreign key or an index uses, so
-- each table is made again with the rows it had (https://sqlite.org/lang_altertable.html#otheralter;
-- db::migrate runs this with foreign keys off, and checks them all before it commits).
--
-- The app only ever used the lowest trongate_users id, and the import makes one user, so a
-- database with more than one user is refused here rather than its rows mixed: this insert
-- fails (CHECK constraint failed: users <= 1), the migration rolls back and the app says so at
-- start, with the database as it was.
CREATE TABLE one_local_user_check (users INTEGER NOT NULL CHECK (users <= 1));
INSERT INTO one_local_user_check (users) SELECT COUNT(*) FROM trongate_users;
DROP TABLE one_local_user_check;

-- A saved match: the job post, its score and when it was rejected. Rows without a user (none
-- the app made) are kept too.
CREATE TABLE matches_of_the_local_user (
  id INTEGER PRIMARY KEY,
  job_title TEXT NOT NULL,
  company TEXT NOT NULL DEFAULT '',
  job_url TEXT DEFAULT NULL,
  job_text TEXT NOT NULL,
  score REAL NOT NULL,
  points INTEGER NOT NULL,
  max_points INTEGER NOT NULL,
  tag TEXT NOT NULL,
  rejected_at INTEGER DEFAULT NULL,
  created_at INTEGER NOT NULL
);
INSERT INTO matches_of_the_local_user
  (id, job_title, company, job_url, job_text, score, points, max_points, tag, rejected_at, created_at)
SELECT id, job_title, company, job_url, job_text, score, points, max_points, tag, rejected_at, created_at
FROM matches;
DROP TABLE matches;
ALTER TABLE matches_of_the_local_user RENAME TO matches;
CREATE INDEX matches_created ON matches (created_at);

-- The candidate behind the local user: at most one, as trongate_user_id was UNIQUE.
CREATE TABLE candidates_of_the_local_user (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  email TEXT NOT NULL UNIQUE,
  phone TEXT DEFAULT NULL,
  postal_code TEXT DEFAULT NULL,
  open_to_work INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
INSERT INTO candidates_of_the_local_user
  (id, name, email, phone, postal_code, open_to_work, created_at, updated_at)
SELECT id, name, email, phone, postal_code, open_to_work, created_at, updated_at
FROM candidates;
DROP TABLE candidates;
ALTER TABLE candidates_of_the_local_user RENAME TO candidates;

-- A saved LLM key per provider (the key itself is in Credential Manager).
CREATE TABLE user_llm_keys_of_the_local_user (
  id INTEGER PRIMARY KEY,
  provider TEXT NOT NULL UNIQUE,
  model TEXT NOT NULL DEFAULT '',
  base_url TEXT NOT NULL DEFAULT '',
  key_hint TEXT NOT NULL DEFAULT '',
  use_for_resume INTEGER NOT NULL DEFAULT 1,
  use_for_application INTEGER NOT NULL DEFAULT 1,
  use_for_job_evaluation INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
INSERT INTO user_llm_keys_of_the_local_user
  (id, provider, model, base_url, key_hint,
   use_for_resume, use_for_application, use_for_job_evaluation, created_at, updated_at)
SELECT id, provider, model, base_url, key_hint,
       use_for_resume, use_for_application, use_for_job_evaluation, created_at, updated_at
FROM user_llm_keys;
DROP TABLE user_llm_keys;
ALTER TABLE user_llm_keys_of_the_local_user RENAME TO user_llm_keys;

-- The model per task (account::task_model::TASKS).
CREATE TABLE user_llm_use_models_of_the_local_user (
  use TEXT PRIMARY KEY CHECK (use IN ('read_job', 'read_cv', 'judge', 'application', 'resume')),
  model TEXT NOT NULL DEFAULT ''
);
INSERT INTO user_llm_use_models_of_the_local_user (use, model)
SELECT use, model FROM user_llm_use_models;
DROP TABLE user_llm_use_models;
ALTER TABLE user_llm_use_models_of_the_local_user RENAME TO user_llm_use_models;

DROP TABLE trongate_users;
