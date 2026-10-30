-- A saved match's tailored résumé head. ?1 the match.
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
FROM cv_match_resumes
WHERE cv_match_id = ?1
