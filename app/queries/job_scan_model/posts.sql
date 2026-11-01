-- A batch of a search's found posts, best first: the saved match's highest score, then newest;
-- posts not matched yet come after the matched ones (score_cursor::NO_SCORE).
-- ?1 the search, ?2 1 to list the dismissed posts too, ?3 what an unmatched post sorts as,
-- ?4 ?5 ?6 the score, found_at and id of the post the batch comes after (all NULL for the first),
-- ?7 how many.
SELECT
  p.id AS "id!",
  p.url,
  p.title,
  p.company,
  p.location,
  p.status,
  p.error,
  p.found_at,
  p.match_id,
  m.score AS "score?: f64",
  m.tag AS "tag?",
  m.rejected_at,
  EXISTS (
    SELECT 1 FROM match_documents md JOIN documents d ON d.id = md.document_id
    WHERE md.match_id = p.match_id AND d.kind = 'application'
  ) AS "has_application!: bool",
  EXISTS (
    SELECT 1 FROM match_documents md JOIN documents d ON d.id = md.document_id
    WHERE md.match_id = p.match_id AND d.kind = 'resume'
  ) AS "has_resume!: bool",
  (
    SELECT i.text
    FROM match_items i
    WHERE i.match_id = m.id AND i.criterion = 'location' AND i.verdict = 'missing'
    ORDER BY i.sort_order
    LIMIT 1
  ) AS "location_missed?: String"
FROM scanned_job_posts p
LEFT JOIN matches m ON m.id = p.match_id
WHERE p.job_search_id = ?1
  AND (?2 = 1 OR p.status <> 'dismissed')
  AND (?4 IS NULL OR (COALESCE(m.score, ?3), p.found_at, p.id) < (?4, ?5, ?6))
ORDER BY COALESCE(m.score, ?3) DESC, p.found_at DESC, p.id DESC
LIMIT ?7
