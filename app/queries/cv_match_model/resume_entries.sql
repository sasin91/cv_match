-- A saved match's tailored résumé roles, projects and schools, in order. ?1 the match.
SELECT id, section, title, organisation, location, starts, ends, summary, note
FROM cv_match_resume_entries
WHERE cv_match_id = ?1
ORDER BY sort_order, id
