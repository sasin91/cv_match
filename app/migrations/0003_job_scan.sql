-- The job scan: saved job searches, the scans of them and the posts they found, as recruiter's
-- db/schema.sql has them in MariaDB ("Job scan"), with cv_match_id as match_id. Left out on the
-- desktop: trongate_user_id (one local user), cv_name and cv_text (every post is matched with
-- the profile, profile::text_for_user), scan_every_hours (how often scans run is the one
-- schedule the setup wizard registers, scan_schedule) and api_token_hash (posting links from
-- ChatGPT, which the desktop doesn't do).

-- A saved job search: what to look for. Each scan (job_scan::scan_service) asks the model,
-- searching the web, for new posts that fit; each post found is matched with the profile, and a
-- post scoring at least tailor_from_score (0-100, 0 = every match) gets its tailored résumé and
-- application written right away. paused_reason says why an active search isn't scanned (no
-- OpenAI key for evaluating job posts, no profile, a key the provider rejected), and is shown on
-- its page and the dashboard. last_scanned_at is when its last scan started.
CREATE TABLE job_searches (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  keywords TEXT NOT NULL,
  location TEXT NOT NULL DEFAULT '',
  remote_ok INTEGER NOT NULL DEFAULT 1,
  tailor_from_score INTEGER NOT NULL DEFAULT 60,
  active INTEGER NOT NULL DEFAULT 1,
  paused_reason TEXT NOT NULL DEFAULT '',
  last_scanned_at INTEGER DEFAULT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX job_searches_created ON job_searches (created_at);

-- The sites a job search is limited to ("jobnet.dk"); a search without any looks on the usual
-- Danish job sites.
CREATE TABLE job_search_sites (
  id INTEGER PRIMARY KEY,
  job_search_id INTEGER NOT NULL REFERENCES job_searches (id) ON DELETE CASCADE,
  domain TEXT NOT NULL,
  UNIQUE (job_search_id, domain)
);

-- One scan of a job search: how many posts it found, how many were new, the model's token use
-- (what the scan cost) and why it failed, if it did. finished_at is NULL while it runs.
CREATE TABLE job_search_runs (
  id INTEGER PRIMARY KEY,
  job_search_id INTEGER NOT NULL REFERENCES job_searches (id) ON DELETE CASCADE,
  started_at INTEGER NOT NULL,
  finished_at INTEGER DEFAULT NULL,
  posts_found INTEGER NOT NULL DEFAULT 0,
  posts_new INTEGER NOT NULL DEFAULT 0,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  error TEXT NOT NULL DEFAULT ''
);
CREATE INDEX job_search_runs_search_started ON job_search_runs (job_search_id, started_at);

-- A job post a scan found (or the user added by its link), and what the auto-match made of it.
-- status: new (waiting), matching, matched (match_id is the saved match), not_a_job (the page
-- doesn't read as a job post), failed (error says why) or dismissed (by the user; dismissing a
-- post rejects its match). url_hash is sha1 of the normalised url (job_search_rules::normal_url),
-- so a search never lists the same post twice.
CREATE TABLE scanned_job_posts (
  id INTEGER PRIMARY KEY,
  job_search_id INTEGER NOT NULL REFERENCES job_searches (id) ON DELETE CASCADE,
  job_search_run_id INTEGER DEFAULT NULL REFERENCES job_search_runs (id) ON DELETE SET NULL,
  url TEXT NOT NULL,
  url_hash TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  company TEXT NOT NULL DEFAULT '',
  location TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'new',
  error TEXT NOT NULL DEFAULT '',
  match_id INTEGER DEFAULT NULL REFERENCES matches (id) ON DELETE SET NULL,
  found_at INTEGER NOT NULL,
  matched_at INTEGER DEFAULT NULL,
  UNIQUE (job_search_id, url_hash)
);
CREATE INDEX scanned_job_posts_search_found ON scanned_job_posts (job_search_id, found_at);
