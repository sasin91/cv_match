-- A batch of the user's saved matches in one list (LISTS), best first: highest score, then newest.
-- ?1 the user, ?2 the integration the Jobnet joblog is kept under, ?3 the list,
-- ?4 the applied statuses as a JSON array (APPLIED_STATUSES),
-- ?5 ?6 ?7 the score, created_at and id of the row the batch comes after (all NULL for the first),
-- ?8 how many.
-- Selects what recent.sql selects, so both read into ListedRow.
SELECT
  m.id,
  m.job_title,
  m.company,
  m.job_url,
  m.cv_name,
  m.score,
  m.points,
  m.max_points,
  m.tag,
  EXISTS (
    SELECT 1 FROM documents d WHERE d.match_id = m.id AND d.kind = 'application'
  ) AS "has_application: bool",
  EXISTS (
    SELECT 1 FROM documents d WHERE d.match_id = m.id AND d.kind = 'resume'
  ) AS "has_resume: bool",
  m.created_at,
  (
    SELECT i.text
    FROM match_items i
    WHERE i.match_id = m.id AND i.criterion = 'location' AND i.verdict = 'missing'
    ORDER BY i.sort_order
    LIMIT 1
  ) AS location_missed,
  j.status AS "jobnet_status?",
  j.logged_at AS jobnet_logged_at,
  m.rejected_at
FROM matches m
LEFT JOIN match_integrations j ON j.match_id = m.id AND j.integration = ?2
WHERE m.trongate_user_id IS ?1
  AND CASE ?3
    -- The top and good ones not applied for yet.
    WHEN 'shortlist' THEN
      m.rejected_at IS NULL
      AND m.tag IN ('top', 'good')
      AND (j.status IS NULL OR j.status NOT IN (SELECT value FROM json_each(?4)))
      AND j.logged_at IS NULL
    -- Applied for, but not yet entered in Jobnet's joblog.
    WHEN 'not_joblogged' THEN
      m.rejected_at IS NULL
      AND j.status IN (SELECT value FROM json_each(?4))
      AND j.logged_at IS NULL
    -- Entered in Jobnet's joblog.
    WHEN 'joblogged' THEN
      m.rejected_at IS NULL
      AND j.logged_at IS NOT NULL
    -- The ones the user rejected.
    WHEN 'rejected' THEN
      m.rejected_at IS NOT NULL
    -- Every other list leaves the rejected ones out.
    ELSE
      m.rejected_at IS NULL
  END
  AND (?5 IS NULL OR (m.score, m.created_at, m.id) < (?5, ?6, ?7))
ORDER BY m.score DESC, m.created_at DESC, m.id DESC
LIMIT ?8
