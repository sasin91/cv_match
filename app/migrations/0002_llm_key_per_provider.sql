-- A saved key per provider instead of one per user, so an OpenAI key, an Anthropic key and
-- local models can be kept side by side and each use run on the one picked for it. Each use is
-- on for one key at most; account_model::save_uses keeps it so. SQLite can't drop the old
-- UNIQUE (trongate_user_id), so the table is made again with the rows it had.
CREATE TABLE user_llm_keys_per_provider (
  id INTEGER PRIMARY KEY,
  trongate_user_id INTEGER NOT NULL REFERENCES trongate_users (id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  model TEXT NOT NULL DEFAULT '',
  base_url TEXT NOT NULL DEFAULT '',
  key_hint TEXT NOT NULL DEFAULT '',
  use_for_resume INTEGER NOT NULL DEFAULT 1,
  use_for_application INTEGER NOT NULL DEFAULT 1,
  use_for_job_evaluation INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (trongate_user_id, provider)
);

INSERT INTO user_llm_keys_per_provider
  (id, trongate_user_id, provider, model, base_url, key_hint,
   use_for_resume, use_for_application, use_for_job_evaluation, created_at, updated_at)
SELECT id, trongate_user_id, provider, model, base_url, key_hint,
       use_for_resume, use_for_application, use_for_job_evaluation, created_at, updated_at
FROM user_llm_keys;

DROP TABLE user_llm_keys;

ALTER TABLE user_llm_keys_per_provider RENAME TO user_llm_keys;
