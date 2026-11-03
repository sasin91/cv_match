-- The devices paired with this app (a phone, a tablet), which reach it through Tailscale
-- (devices). name is what the device called itself when it paired. Its token is kept in the
-- system's credential store (account::key_vault), never here. last_seen_at is its last request,
-- NULL until it sends one after pairing.
CREATE TABLE devices (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  paired_at INTEGER NOT NULL,
  last_seen_at INTEGER DEFAULT NULL
);
