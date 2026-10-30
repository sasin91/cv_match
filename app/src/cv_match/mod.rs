//! cv_match: matching a job post against a CV, ported from modules/cv_match.
//!
//! A job post and a CV come in the shapes of the language model's extract_job and extract_cv
//! answers, whether the model read them or Free_reader did, so Cv_matcher scores both the same.

pub mod cv_matcher;
pub mod free_reader;
pub mod match_score;

use serde::{Deserialize, Serialize};

use crate::taxonomy::Method;

/// A job post as extract_job answers it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    pub text_en: String,
    pub job_title: String,
    pub job_title_en: String,
    pub heading: String,
    pub company: String,
    pub requirements: Vec<Requirement>,
    pub responsibilities: Vec<String>,
}

/// One thing a job post asks for. Requirements sharing an `alt_group` are alternatives
/// ("pædagog eller pædagogisk assistent").
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Requirement {
    pub value: String,
    pub english: String,
    /// skill, education, certificate, experience, language, soft_skill or location.
    pub kind: String,
    pub required: bool,
    pub alt_group: String,
    pub min_years: i64,
    pub min_level: String,
}

/// A CV as extract_cv answers it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub text_en: String,
    pub title: Vec<String>,
    pub skill: Vec<String>,
    pub language: Vec<String>,
    pub certificate: Vec<String>,
    pub education: Vec<String>,
    pub experience_years: i64,
    pub responsibilities: Vec<String>,
}

/// A taxonomy term a phrase is known to name: its position in Taxonomy::terms and how the
/// phrase matched it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnownTerm {
    pub index: usize,
    pub method: Method,
}
