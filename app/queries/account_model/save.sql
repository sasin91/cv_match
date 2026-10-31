INSERT INTO user_llm_keys (trongate_user_id, provider, model, base_url, key_hint, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
ON CONFLICT (trongate_user_id) DO UPDATE SET
  provider = excluded.provider,
  model = excluded.model,
  base_url = excluded.base_url,
  key_hint = excluded.key_hint,
  updated_at = excluded.updated_at
