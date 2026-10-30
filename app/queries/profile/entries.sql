-- The candidate's profile entries by section, newest first: ongoing ones on top, then by when
-- they ended and started; the CV's order breaks ties. ?1 the candidate.
SELECT id, section, title, organisation, location, url, started_on, ended_on, summary, note
FROM candidate_profile_entries
WHERE candidate_id = ?1
ORDER BY
  CASE section WHEN 'experience' THEN 1 WHEN 'project' THEN 2 WHEN 'education' THEN 3 ELSE 0 END,
  ended_on IS NULL DESC,
  ended_on DESC,
  started_on DESC,
  sort_order,
  id
