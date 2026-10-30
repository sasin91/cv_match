-- The user's newest saved matches not rejected, for the extension's history.
-- ?1 the user, ?2 the integration the Jobnet joblog is kept under, ?3 how many.
-- Selects what best_first.sql selects, so both read into ListedRow.
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
    SELECT 1 FROM documents d WHERE d.cv_match_id = m.id AND d.kind = 'application'
  ) AS "has_application: bool",
  EXISTS (
    SELECT 1 FROM documents d WHERE d.cv_match_id = m.id AND d.kind = 'resume'
  ) AS "has_resume: bool",
  m.created_at,
  (
    SELECT i.text
    FROM cv_match_items i
    WHERE i.cv_match_id = m.id AND i.criterion = 'location' AND i.verdict = 'missing'
    ORDER BY i.sort_order
    LIMIT 1
  ) AS location_missed,
  j.status AS "jobnet_status?",
  j.logged_at AS jobnet_logged_at,
  m.rejected_at
FROM cv_matches m
LEFT JOIN cv_match_integrations j ON j.cv_match_id = m.id AND j.integration = ?2
WHERE m.trongate_user_id IS ?1 AND m.rejected_at IS NULL
ORDER BY m.created_at DESC, m.id DESC
LIMIT ?3
