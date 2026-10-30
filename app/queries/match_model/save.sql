-- A new saved match: the post and the CV it was matched with, and its score. ?1 the user,
-- ?2 job_title, ?3 company, ?4 job_url, ?5 job_text, ?6 cv_name, ?7 cv_text, ?8 score,
-- ?9 points, ?10 max_points, ?11 tag, ?12 created_at.
INSERT INTO matches
  (trongate_user_id, job_title, company, job_url, job_text, cv_name, cv_text, score, points,
   max_points, tag, created_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
