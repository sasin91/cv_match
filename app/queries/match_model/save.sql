-- A new saved match: the post and its score; the CV it was matched with is linked to it after
-- (match_documents). ?1 the user, ?2 job_title, ?3 company, ?4 job_url, ?5 job_text, ?6 score,
-- ?7 points, ?8 max_points, ?9 tag, ?10 created_at.
INSERT INTO matches
  (trongate_user_id, job_title, company, job_url, job_text, score, points, max_points, tag,
   created_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
