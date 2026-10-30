-- A tailored résumé's head. ?1 the résumé (documents.id).
SELECT
  name,
  title,
  location,
  phone,
  email,
  links,
  intro,
  education_note,
  experience_heading,
  skills_heading,
  education_heading,
  languages_heading,
  projects_heading
FROM match_resumes
WHERE document_id = ?1
