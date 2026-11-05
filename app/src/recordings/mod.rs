//! Recordings: a call, a voice message or a spoken note about a saved match, turned into text on
//! this computer by Whisper (whisper), and kept with its timed lines (recording_model). The audio
//! file is kept in the database's own folder under recordings, next to it (folder_for), so it can be played again; Delete
//! removes both.
//!
//! A recording comes in as a file picked on the match page (recording_service::add_file). The
//! phone's call recorder's files reach the computer by Taildrop; picking them up from there is
//! the next step.

pub mod audio;
pub mod recording_model;
pub mod recording_row;
pub mod recording_service;

use std::path::{Path, PathBuf};

/// The recordings folder's name next to the database.
pub const FOLDER: &str = "recordings";

/// Where database's recordings are kept: a folder of its own, named after the database file,
/// inside the recordings folder next to it (recordings\jobfinder for jobfinder.sqlite), so two
/// databases in one folder never share, or delete, each other's audio.
pub fn folder_for(database: &Path) -> PathBuf {
    let data_folder = database.parent().unwrap_or(Path::new("."));
    let own = database.file_stem().unwrap_or(database.as_os_str());
    data_folder.join(FOLDER).join(own)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_database_keeps_its_recordings_apart() {
        let main = folder_for(Path::new("/home/me/jobfinder.sqlite"));
        let other = folder_for(Path::new("/home/me/test.sqlite"));
        assert_eq!(main, Path::new("/home/me/recordings/jobfinder"));
        assert_eq!(other, Path::new("/home/me/recordings/test"));
    }
}
