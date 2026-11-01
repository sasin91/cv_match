-- The newest version of one kind of a saved match's documents, the one the page shows.
-- ?1 the match, ?2 the kind (application, resume).
SELECT d.id, d.text, d.written_at
FROM documents d
JOIN match_documents md ON md.document_id = d.id
WHERE md.match_id = ?1 AND d.kind = ?2
ORDER BY d.written_at DESC, d.id DESC
LIMIT 1
