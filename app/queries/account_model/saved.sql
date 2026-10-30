SELECT provider, model, base_url, key_hint,
       use_for_resume, use_for_application, use_for_job_evaluation,
       updated_at
FROM user_llm_keys
WHERE trongate_user_id = ?1
