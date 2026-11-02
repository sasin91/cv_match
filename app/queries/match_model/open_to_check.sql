-- The user's saved matches the still-open check looks at (matches::still_open): not rejected,
-- not found closed, and not said to be open by the user. ?1 what closed_why says
-- when the user said a job is still open.
SELECT
  m.id,
  m.job_url,
  m.job_text,
  m.created_at,
  m.deadline,
  m.still_open_checked_at
FROM matches m
WHERE m.rejected_at IS NULL
  AND m.closed_at IS NULL
  AND m.closed_why != ?1
ORDER BY m.created_at DESC, m.id DESC
