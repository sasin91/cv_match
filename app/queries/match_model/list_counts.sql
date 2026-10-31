-- How many of the user's saved matches each list (LISTS) holds, picked as best_first.sql picks
-- them. ?1 the user, ?2 the integration the Jobnet joblog is kept under, ?3 the applied
-- statuses as a JSON array (APPLIED_STATUSES).
SELECT
  COALESCE(SUM(
    m.rejected_at IS NULL
    AND m.tag IN ('top', 'good')
    AND (j.status IS NULL OR j.status NOT IN (SELECT value FROM json_each(?3)))
    AND j.logged_at IS NULL
  ), 0) AS "shortlist!: i64",
  COALESCE(SUM(m.rejected_at IS NULL), 0) AS "every!: i64",
  COALESCE(SUM(
    m.rejected_at IS NULL
    AND j.status IN (SELECT value FROM json_each(?3))
    AND j.logged_at IS NULL
  ), 0) AS "not_joblogged!: i64",
  COALESCE(SUM(m.rejected_at IS NULL AND j.logged_at IS NOT NULL), 0) AS "joblogged!: i64",
  COALESCE(SUM(m.rejected_at IS NOT NULL), 0) AS "rejected!: i64"
FROM matches m
LEFT JOIN match_integrations j ON j.match_id = m.id AND j.integration = ?2
WHERE m.trongate_user_id IS ?1
