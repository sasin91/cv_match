-- A tailored résumé's roles, projects and schools, in order. ?1 the résumé (documents.id).
SELECT id, section, title, organisation, location, starts, ends, summary, note
FROM match_resume_entries
WHERE document_id = ?1
ORDER BY sort_order, id
