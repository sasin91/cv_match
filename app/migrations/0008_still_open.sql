-- Whether a saved match's job still takes applications, as the still-open check
-- (matches::still_open) last found it.
-- deadline is the post's application deadline (yyyy-mm-dd) read from its text, NULL when it gives
-- none. still_open_checked_at is when the post's page was last read for it. closed_at is when the
-- job was first found closed, NULL while it is open; closed_why says what gave it away: deadline
-- (it passed), gone (the post was taken down), says_closed (its page says so) or moved (its link
-- now lands on the site's front or search page). by_hand_open (with closed_at NULL) is the user
-- saying it is still open, and the check leaves it alone after that.
ALTER TABLE matches ADD COLUMN deadline TEXT DEFAULT NULL;
ALTER TABLE matches ADD COLUMN still_open_checked_at INTEGER DEFAULT NULL;
ALTER TABLE matches ADD COLUMN closed_at INTEGER DEFAULT NULL;
ALTER TABLE matches ADD COLUMN closed_why TEXT NOT NULL DEFAULT '';
