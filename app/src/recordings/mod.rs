//! Recordings: a call, a voice message or a spoken note about a saved match, turned into text on
//! this computer by Whisper (whisper), and kept with its timed lines (recording_model). The audio
//! file is kept in the recordings folder next to the database, so it can be played again; Delete
//! removes both.
//!
//! A recording comes in as a file picked on the match page (recording_service::add_file). The
//! phone's call recorder's files reach the computer by Taildrop; picking them up from there is
//! the next step.

pub mod audio;
pub mod recording_model;
pub mod recording_row;
pub mod recording_service;

/// The recordings folder's name next to the database.
pub const FOLDER: &str = "recordings";
