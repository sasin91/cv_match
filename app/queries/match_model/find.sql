-- One of the user's saved matches, with the CV it was matched with. ?1 the match, ?2 the user.
SELECT
  m.id,
  m.trongate_user_id,
  m.job_title,
  m.company,
  m.job_url,
  m.job_text,
  COALESCE(cv.name, '') AS "cv_name!: String",
  COALESCE(cv.text, '') AS "cv_text!: String",
  m.score,
  m.points,
  m.max_points,
  m.tag,
  m.rejected_at,
  m.created_at
FROM matches m
LEFT JOIN (
  SELECT md.match_id, d.name, d.text
  FROM match_documents md
  JOIN documents d ON d.id = md.document_id AND d.kind = 'cv'
) cv ON cv.match_id = m.id
WHERE m.id = ?1 AND m.trongate_user_id IS ?2
