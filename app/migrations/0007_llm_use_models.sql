-- A model per task, so each task runs on its own model on the key its use picks
-- (account::task_model::TASKS): read_job, read_cv and judge on the job_evaluation key,
-- application and resume on theirs. '' (or no row) is the key's own model, or for this app's own
-- model the task's default when its file is in the models folder.
CREATE TABLE user_llm_use_models (
  trongate_user_id INTEGER NOT NULL REFERENCES trongate_users (id) ON DELETE CASCADE,
  use TEXT NOT NULL CHECK (use IN ('read_job', 'read_cv', 'judge', 'application', 'resume')),
  model TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (trongate_user_id, use)
);
