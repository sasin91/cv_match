-- The job searches, newest first, each with how many posts it found (not counting dismissed
-- ones) and how many of those are matched.
SELECT
  s.id,
  s.name,
  s.keywords,
  s.location,
  s.active,
  s.paused_reason,
  s.last_scanned_at,
  COUNT(p.id) AS "posts!: i64",
  COALESCE(SUM(p.status = 'matched'), 0) AS "matched!: i64"
FROM job_searches s
LEFT JOIN scanned_job_posts p ON p.job_search_id = s.id AND p.status <> 'dismissed'
GROUP BY s.id
ORDER BY s.created_at DESC, s.id DESC
