//! JobFinder: Recruiter's CV match as a local desktop app. tests/parity.rs checks the matching context's answers
//! against the fixed expected answers in tests/fixtures/parity.json.

pub mod account;
pub mod app_log;
pub mod cleanup;
pub mod context;
pub mod dashboard;
pub mod db;
pub mod desktop;
pub mod dev_reload;
pub mod extension;
pub mod from_recruiter;
pub mod instance;
pub mod job_scan;
pub mod laya;
pub mod llama;
pub mod llm;
pub mod matches;
pub mod no_window;
pub mod pdf;
pub mod pdf_text;
pub mod profile;
pub mod scan_schedule;
pub mod server;
pub mod service_error;
pub mod setup;
pub mod taxonomy;
