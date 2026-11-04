-- The app's secrets (account::key_vault): each provider's API key and each paired device's token
-- hash. Each value is sealed with XChaCha20-Poly1305 under the database's data key, which is kept
-- in its own file beside the database (`<database>-key`), never in this one, so a copy of the
-- database alone opens none of them. name says which secret it is (llm_key:openai,
-- device_token_hash:3); sealed is a version byte, a random 24-byte nonce and the ciphertext, with
-- name bound in as associated data, so a value moved to another name doesn't open.
CREATE TABLE secrets (
  name TEXT PRIMARY KEY,
  sealed BLOB NOT NULL,
  updated_at INTEGER NOT NULL
);
