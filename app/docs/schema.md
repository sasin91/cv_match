# Database schema

The tables after every migration in `migrations/` (up to `0011_one_user_schema`), as SQLite has them. PK is a primary key, FK a foreign key; a line runs from the table a key points at to the table that has it.

```mermaid
erDiagram
  documents {
    integer id PK
    text kind
    text name
    text text
    text written_by
    text notes
    integer written_at
  }
  job_search_run_steps {
    integer id PK
    integer job_search_run_id FK
    integer scanned_job_post_id FK
    text kind
    text text
    integer failed
    integer started_at_ms
    integer took_ms
    text asked
    text answered
  }
  job_search_runs {
    integer id PK
    integer job_search_id FK
    integer started_at
    integer finished_at
    integer posts_found
    integer posts_new
    integer input_tokens
    integer output_tokens
    text error
  }
  job_search_sites {
    integer id PK
    integer job_search_id FK
    text domain
  }
  job_searches {
    integer id PK
    text name
    text keywords
    text location
    integer remote_ok
    integer tailor_from_score
    integer active
    text paused_reason
    integer last_scanned_at
    integer created_at
    integer updated_at
  }
  llm_keys {
    integer id PK
    text provider
    text model
    text base_url
    text key_hint
    integer use_for_resume
    integer use_for_application
    integer use_for_job_evaluation
    integer created_at
    integer updated_at
  }
  llm_task_models {
    text task PK
    text model
  }
  match_documents {
    integer match_id PK,FK
    integer document_id PK,FK
  }
  match_integrations {
    integer id PK
    integer match_id FK
    text integration
    text status
    text external_id
    integer applied_at
    integer logged_at
    integer created_at
    integer updated_at
  }
  match_items {
    integer id PK
    integer match_id FK
    text criterion
    text text
    text verdict
    text decided_by
    text reason
    text evidence
    integer sort_order
  }
  match_rejections {
    integer id PK
    integer match_id FK
    text reason
    text note
    integer created_at
  }
  match_resume_entries {
    integer id PK
    integer document_id FK
    text section
    text title
    text organisation
    text location
    text starts
    text ends
    text summary
    text note
    integer sort_order
  }
  match_resume_lines {
    integer id PK
    integer document_id FK
    integer entry_id FK
    text kind
    text text
    integer sort_order
  }
  match_resumes {
    integer document_id PK,FK
    text name
    text title
    text location
    text phone
    text email
    text links
    text intro
    text education_note
    text experience_heading
    text skills_heading
    text education_heading
    text languages_heading
    text projects_heading
  }
  matches {
    integer id PK
    text job_title
    text company
    text job_url
    text job_text
    real score
    integer points
    integer max_points
    text tag
    integer rejected_at
    integer created_at
    text deadline
    integer still_open_checked_at
    integer closed_at
    text closed_why
  }
  profile {
    integer id PK
    text name
    text email
    text phone
    text title
    text location
    text about
    text filled_from_cv_name
    integer filled_from_cv_at
    integer created_at
    integer updated_at
  }
  profile_entries {
    integer id PK
    text section
    text title
    text organisation
    text location
    text url
    text started_on
    text ended_on
    text summary
    text note
    integer sort_order
    integer created_at
    integer updated_at
  }
  profile_lines {
    integer id PK
    integer entry_id FK
    text kind
    text text
    integer sort_order
  }
  profile_links {
    integer id PK
    text label
    text url
    integer position
    integer created_at
    integer updated_at
  }
  scanned_job_posts {
    integer id PK
    integer job_search_id FK
    integer job_search_run_id FK
    text url
    text url_hash
    text title
    text company
    text location
    text status
    text error
    integer match_id FK
    integer found_at
    integer matched_at
    integer matching_since
  }
  taxonomy_label_sightings {
    integer id PK
    integer label_id FK
    integer match_id FK
    text learned_from
    text model
    integer seen_at
  }
  taxonomy_labels {
    integer id PK
    integer term_id FK
    text text
    text lang
    integer preferred
    text source
    text status
    integer created_at
    integer updated_at
  }
  taxonomy_shipped {
    integer id PK
    text index_sha256
    integer written_at
  }
  taxonomy_terms {
    integer id PK
    text kind
    integer parent_id FK
    text definition
    text source
  }
  scanned_job_posts |o--o{ job_search_run_steps : "scanned_job_post_id"
  job_search_runs ||--o{ job_search_run_steps : "job_search_run_id"
  job_searches ||--o{ job_search_runs : "job_search_id"
  job_searches ||--o{ job_search_sites : "job_search_id"
  documents ||--o{ match_documents : "document_id"
  matches ||--o{ match_documents : "match_id"
  matches ||--o{ match_integrations : "match_id"
  matches ||--o{ match_items : "match_id"
  matches ||--o{ match_rejections : "match_id"
  documents ||--o{ match_resume_entries : "document_id"
  match_resume_entries |o--o{ match_resume_lines : "entry_id"
  documents ||--o{ match_resume_lines : "document_id"
  documents ||--o| match_resumes : "document_id"
  profile_entries |o--o{ profile_lines : "entry_id"
  matches |o--o{ scanned_job_posts : "match_id"
  job_search_runs |o--o{ scanned_job_posts : "job_search_run_id"
  job_searches ||--o{ scanned_job_posts : "job_search_id"
  matches ||--o{ taxonomy_label_sightings : "match_id"
  taxonomy_labels ||--o{ taxonomy_label_sightings : "label_id"
  taxonomy_terms ||--o{ taxonomy_labels : "term_id"
  taxonomy_terms |o--o{ taxonomy_terms : "parent_id"
```
