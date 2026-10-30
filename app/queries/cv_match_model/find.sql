-- One of the user's saved matches. ?1 the match, ?2 the user.
SELECT
  id,
  trongate_user_id,
  job_title,
  company,
  job_url,
  job_text,
  cv_name,
  cv_text,
  score,
  points,
  max_points,
  tag,
  rejected_at,
  created_at
FROM cv_matches
WHERE id = ?1 AND trongate_user_id IS ?2
