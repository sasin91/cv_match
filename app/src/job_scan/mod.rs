//! job_scan: saved job searches, scanned for new job posts on the job scan's schedule (`recruiter
//! scan`, which Task Scheduler or the systemd timer runs) or with Scan now, as recruiter's
//! job_scan module does without its ChatGPT, API and MCP routes. Each post found is matched with
//! the profile and saved with the matches; a good match gets its tailored résumé and application
//! right away.
//!
//! The work is in scan_service (the scan) and search_service (the searches and their posts), which
//! the Job searches screen calls.

pub mod found_post_card;
pub mod job_scan_model;
pub mod job_search_card;
pub mod job_search_rules;
pub mod job_search_summary;
pub mod run_steps;
pub mod scan_lock;
pub mod scan_service;
pub mod scan_timeline;
pub mod search_service;
