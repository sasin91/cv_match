-- The job scan's timeline: what one scan of a job search did, step by step, so a search's page
-- shows it live while the scan runs (Scan now, or the scheduled `jobfinder scan` in a process
-- of its own) and afterwards. A step is a site read for a search term, the web search, the posts
-- listed, or, for one post (scanned_job_post_id), reading its page, reading it with the model,
-- the match and the writing. A post's steps are on the run that found it.
--
-- kind: site, web, posts, read, extract, match or write. text is the step as the timeline says
-- it ("Jobnet, PHP udvikler near Odense: 20 posts"); failed marks one that went wrong.
-- started_at_ms and took_ms are in milliseconds (unix time). asked and answered are what the
-- model was asked and answered in the step (llm::transcript), every call one after the other,
-- '' for a step without the model; the page reads them only when one is opened. Only each
-- search's latest scans keep their steps (job_scan_model::RUNS_WITH_STEPS).
CREATE TABLE job_search_run_steps (
  id INTEGER PRIMARY KEY,
  job_search_run_id INTEGER NOT NULL REFERENCES job_search_runs (id) ON DELETE CASCADE,
  scanned_job_post_id INTEGER DEFAULT NULL REFERENCES scanned_job_posts (id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  failed INTEGER NOT NULL DEFAULT 0,
  started_at_ms INTEGER NOT NULL,
  took_ms INTEGER NOT NULL DEFAULT 0,
  asked TEXT NOT NULL DEFAULT '',
  answered TEXT NOT NULL DEFAULT ''
);
CREATE INDEX job_search_run_steps_run ON job_search_run_steps (job_search_run_id, id);
CREATE INDEX job_search_run_steps_post ON job_search_run_steps (scanned_job_post_id);
