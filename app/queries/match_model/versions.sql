-- Every version of a saved match's documents, newest first in the order newest_document.sql
-- picks the newest, for the "Earlier versions" lists. ?1 the match. The CV it was matched
-- with has no versions of its own, so it isn't one.
SELECT d.id, d.kind, d.text, d.written_by, d.notes, d.written_at
FROM documents d
JOIN match_documents md ON md.document_id = d.id
WHERE md.match_id = ?1 AND d.kind != 'cv'
ORDER BY d.written_at DESC, d.id DESC
