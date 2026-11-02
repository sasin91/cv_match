-- The matches that taught a label, oldest first, with where each was found (the saved match's
-- post) and why: the match's item for the requirement the label is (its text, or its text with
-- the " (level)" the matcher adds), whose verdict, evidence and reason the judge gave.
SELECT
  s.match_id AS "match_id!",
  s.learned_from AS "learned_from!",
  s.model AS "model!",
  s.seen_at AS "seen_at!",
  m.job_title AS "job_title!",
  m.company AS "company!",
  m.job_url AS "job_url: String",
  i.verdict AS "verdict: String",
  i.decided_by AS "decided_by: String",
  i.evidence AS "evidence: String",
  i.reason AS "reason: String"
FROM taxonomy_label_sightings s
JOIN taxonomy_labels l ON l.id = s.label_id
JOIN matches m ON m.id = s.match_id
LEFT JOIN match_items i ON i.id = (
  SELECT i2.id FROM match_items i2
  WHERE i2.match_id = s.match_id
    AND (i2.text = l.text OR substr(i2.text, 1, length(l.text) + 2) = l.text || ' (')
  ORDER BY i2.sort_order, i2.id LIMIT 1)
WHERE s.label_id = ?1
ORDER BY s.seen_at, s.id
