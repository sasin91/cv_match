-- Every label, shipped or learned, whose text contains ?1 (a LIKE pattern escaped with \),
-- case aside, shortest first, in the Learned terms screen's shape.
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
WHERE l.text LIKE ?1 ESCAPE '\'
ORDER BY length(l.text), l.id
LIMIT ?2
