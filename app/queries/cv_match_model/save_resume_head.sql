-- A tailored résumé's head. ?1 the résumé (documents.id), then its fields in the columns' order.
INSERT INTO cv_match_resumes (
  document_id,
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
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
