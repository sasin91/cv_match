-- The newest version of one kind of a saved match's documents, the one the page shows.
-- ?1 the match, ?2 the kind (application, resume).
SELECT id, text, written_at
FROM documents
WHERE match_id = ?1 AND kind = ?2
ORDER BY written_at DESC, id DESC
LIMIT 1
