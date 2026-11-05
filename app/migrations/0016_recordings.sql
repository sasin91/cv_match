-- Recordings (recordings): a call, a voice message or a spoken note, transcribed on this computer
-- by Whisper (whisper). Columns take schema.org's AudioObject / MediaObject / CreativeWork
-- property names where one fits: match_id is what it is about, content_url the audio file's name
-- in the recordings folder next to the database, encoding_format its MIME type, content_size its
-- bytes, duration_ms its length (schema.org duration, kept in milliseconds), date_created when
-- it was recorded (the file's time) and upload_date when the app got it (unix seconds),
-- in_language the language Whisper heard (BCP 47, e.g. da), abstract the short summary.
-- kind is call, voice_message or note; source is file or taildrop. transcript, transcribed_by
-- and transcribed_at are NULL until it is transcribed.
CREATE TABLE recordings (
  id INTEGER PRIMARY KEY,
  match_id INTEGER DEFAULT NULL REFERENCES matches (id) ON DELETE SET NULL,
  kind TEXT NOT NULL,
  source TEXT NOT NULL,
  content_url TEXT NOT NULL,
  encoding_format TEXT DEFAULT NULL,
  content_size INTEGER DEFAULT NULL,
  duration_ms INTEGER DEFAULT NULL,
  date_created INTEGER DEFAULT NULL,
  upload_date INTEGER NOT NULL,
  in_language TEXT DEFAULT NULL,
  transcript TEXT DEFAULT NULL,
  abstract TEXT DEFAULT NULL,
  transcribed_by TEXT DEFAULT NULL,
  transcribed_at INTEGER DEFAULT NULL
);
CREATE INDEX recordings_match ON recordings (match_id);

-- One timed line of a recording's transcript (schema.org Clip startTime / endTime, in
-- milliseconds from the start), so a line can be played from where it was said.
CREATE TABLE recording_segments (
  recording_id INTEGER NOT NULL REFERENCES recordings (id) ON DELETE CASCADE,
  start_time_ms INTEGER NOT NULL,
  end_time_ms INTEGER NOT NULL,
  text TEXT NOT NULL,
  PRIMARY KEY (recording_id, start_time_ms)
);
