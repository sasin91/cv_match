-- A new provider's key takes the uses no other key of the user's is on for; a key saved again
-- keeps its own.
INSERT INTO user_llm_keys (
  trongate_user_id, provider, model, base_url, key_hint,
  use_for_resume, use_for_application, use_for_job_evaluation, created_at, updated_at
)
VALUES (
  ?1, ?2, ?3, ?4, ?5,
  NOT EXISTS (SELECT 1 FROM user_llm_keys WHERE trongate_user_id = ?1 AND use_for_resume = 1),
  NOT EXISTS (SELECT 1 FROM user_llm_keys WHERE trongate_user_id = ?1 AND use_for_application = 1),
  NOT EXISTS (SELECT 1 FROM user_llm_keys WHERE trongate_user_id = ?1 AND use_for_job_evaluation = 1),
  ?6, ?6
)
ON CONFLICT (trongate_user_id, provider) DO UPDATE SET
  model = excluded.model,
  base_url = excluded.base_url,
  key_hint = excluded.key_hint,
  updated_at = excluded.updated_at
