-- Gaming mode (account::gaming_mode): while the user plays, work that would run on a model on
-- this computer runs on their cloud key instead, so the game keeps the graphics card. No row is
-- on.
CREATE TABLE user_gaming_mode (
  trongate_user_id INTEGER PRIMARY KEY REFERENCES trongate_users (id) ON DELETE CASCADE,
  -- 1: while a game runs, the work goes to the cloud key; 0: it waits until the game ends.
  offload INTEGER NOT NULL CHECK (offload IN (0, 1))
);
