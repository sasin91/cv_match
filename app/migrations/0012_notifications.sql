-- What the app told the user, for the window's bell (notifications): a good match a scan found,
-- a scan that finished or failed, saved jobs that closed. kind is good_match, scan_finished,
-- scan_failed or jobs_closed. link is the place in the window it opens ("match/12",
-- "job_scan"). read_at is when the bell was opened on it, NULL until then. Rows older than
-- notification_model::KEEP_DAYS are deleted at start.
CREATE TABLE notifications (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  text TEXT NOT NULL DEFAULT '',
  link TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  read_at INTEGER DEFAULT NULL
);
CREATE INDEX notifications_created ON notifications (created_at);
