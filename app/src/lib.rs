//! Recruiter's cv_match as a local desktop app. The matching core is ported from the PHP
//! modules file by file, with the same names, and tests/parity.rs checks it answers as PHP does.

pub mod cv_match;
pub mod db;
pub mod import;
pub mod php;
pub mod profile;
pub mod server;
pub mod taxonomy;
