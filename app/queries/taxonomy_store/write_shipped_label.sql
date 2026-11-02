-- A shipped label, active. A learned row with the same text that was rejected (a shipped label
-- edited or rejected by hand) keeps it out; a learned candidate or active one becomes shipped.
INSERT INTO taxonomy_labels (term_id, text, lang, preferred, source, status, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, 'shipped', 'active', ?5, ?5)
ON CONFLICT (term_id, text, lang) DO UPDATE SET
  source = 'shipped',
  status = 'active',
  preferred = excluded.preferred,
  updated_at = excluded.updated_at
WHERE taxonomy_labels.status <> 'rejected'
