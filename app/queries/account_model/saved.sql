SELECT provider, model, base_url, key_hint,
       use_for_resume, use_for_application, use_for_job_evaluation,
       updated_at
FROM llm_keys
ORDER BY created_at, id
