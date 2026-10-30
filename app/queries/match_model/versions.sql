-- Every version of a saved match's documents, newest first in the order newest_document.sql
-- picks the newest, for the "Earlier versions" lists. ?1 the match.
SELECT id, kind, text, written_by, notes, written_at
FROM documents
WHERE match_id = ?1
ORDER BY written_at DESC, id DESC
