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
  application_text,
  application_written_at,
  resume_text,
  resume_written_at,
  rejected_at,
  created_at,
  EXISTS (
    SELECT 1 FROM cv_match_resumes r WHERE r.cv_match_id = cv_matches.id
  ) AS "resume_structured: bool"
FROM cv_matches
WHERE id = ?1 AND trongate_user_id IS ?2
