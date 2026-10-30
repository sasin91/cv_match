-- Saves a match's status in an outside service. ?1 the match, ?2 the integration, ?3 the status,
-- ?4 when it was applied for (NULL unless ?3 is an applied status), ?5 when it was logged there
-- (NULL unless it is), ?6 now, ?7 whether ?3 is an applied status, ?8 whether it is logged.
-- The first applied status and the first time logged keep their dates; any other status clears
-- applied_at.
INSERT INTO cv_match_integrations
  (cv_match_id, integration, status, applied_at, logged_at, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
ON CONFLICT (cv_match_id, integration) DO UPDATE SET
  status = excluded.status,
  updated_at = excluded.updated_at,
  applied_at = CASE WHEN ?7 THEN COALESCE(applied_at, excluded.applied_at) END,
  logged_at = CASE WHEN ?8 THEN COALESCE(logged_at, excluded.logged_at) END
