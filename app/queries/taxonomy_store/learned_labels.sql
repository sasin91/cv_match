-- The learned labels for the Learned terms screen: candidates first, then the newest changed.
-- term_label is the term's preferred label, Danish first; the first sighting is the oldest.
SELECT
  l.id AS "id!",
  l.term_id AS "term_id!",
  COALESCE((SELECT p.text FROM taxonomy_labels p
            WHERE p.term_id = l.term_id AND p.preferred = 1 AND p.status = 'active'
            ORDER BY p.lang <> 'da' LIMIT 1), '') AS "term_label!: String",
  l.text AS "text!",
  l.lang AS "lang!",
  l.source AS "source!",
  l.status AS "status!",
  (SELECT COUNT(*) FROM taxonomy_label_sightings s WHERE s.label_id = l.id) AS "sightings!: i64",
  (SELECT s.match_id FROM taxonomy_label_sightings s WHERE s.label_id = l.id
   ORDER BY s.seen_at, s.id LIMIT 1) AS "first_match_id: i64",
  (SELECT MIN(s.seen_at) FROM taxonomy_label_sightings s WHERE s.label_id = l.id) AS "first_seen_at: i64"
FROM taxonomy_labels l
WHERE l.source = 'learned'
ORDER BY l.status <> 'candidate', l.updated_at DESC, l.id DESC
LIMIT ?1
