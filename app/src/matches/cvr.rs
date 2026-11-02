//! The company's registered address in the CVR register (cvrapi.dk), for a joblog entry whose
//! post names no postcode and town. It is the registered address, which may not be where the job
//! is, so the joblog marks it as such (note). Only a company whose name shares most words with
//! the post's is taken (same_company); a failed or slow lookup gives nothing.

use std::time::Duration;

use serde_json::Value;

use super::application_delivery::rawurlencode;

const CVRAPI: &str = "https://cvrapi.dk/api";
/// A slow register must not hold up the joblog: after this, it is shown without.
const TIMEOUT: Duration = Duration::from_secs(4);
/// cvrapi.dk asks every caller to say who it is.
const USER_AGENT: &str = "JobFinder - joblog address lookup";
/// "A/S", "ApS" and the like at the end of a name: the register writes them its own way.
const LEGAL_FORMS: [&str; 8] = ["a/s", "aps", "i/s", "p/s", "k/s", "ivs", "amba", "a.m.b.a."];
/// Words left out when names are compared: legal forms, and what is left of "A/S" split up.
const NOT_NAME_WORDS: [&str; 9] = ["as", "aps", "is", "ps", "ks", "ivs", "amba", "a", "s"];
/// The share of the post's name words the register's name must have.
const SAME_NAME_SHARE: f64 = 0.5;

/// A registered address: the register's name for the company, its street address, and
/// "postcode town".
#[derive(Debug, Clone, PartialEq)]
pub struct CvrAddress {
    pub name: String,
    pub address: String,
    pub postal_code_and_city: String,
}

impl CvrAddress {
    /// The hint under a field filled from the register.
    pub fn note(&self) -> String {
        format!(
            "From the CVR register ({}): the registered address, check that it's where the job is",
            self.name
        )
    }
}

/// The registered address of company, or None when the register has no company by that name or
/// can't be reached.
pub async fn registered_address(company: &str) -> Option<CvrAddress> {
    let company = company.trim();
    if company.is_empty() {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .ok()?;
    let answer = client
        .get(format!(
            "{CVRAPI}?search={}&country=dk",
            rawurlencode(without_legal_form(company))
        ))
        .header("Accept", "application/json")
        .send()
        .await;
    let found: Value = match answer {
        Ok(answer) => answer.json().await.ok()?,
        Err(e) => {
            crate::log_line!("CVR register: {company}: {e}");
            return None;
        }
    };
    address_of(company, &found)
}

/// The address in the register's answer, when it is company's.
fn address_of(company: &str, found: &Value) -> Option<CvrAddress> {
    if found.get("error").is_some_and(|error| !error.is_null()) {
        return None;
    }
    let text = |key: &str| match found.get(key) {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    };
    let (name, zipcode, city) = (text("name"), text("zipcode"), text("city"));
    if zipcode.is_empty() || city.is_empty() || !same_company(company, &name) {
        return None;
    }
    Some(CvrAddress {
        address: text("address"),
        postal_code_and_city: format!("{zipcode} {city}"),
        name,
    })
}

fn without_legal_form(company: &str) -> &str {
    let lower = company.to_lowercase();
    for form in LEGAL_FORMS {
        for ending in [form.to_string(), format!("{form}.")] {
            if let Some(start) = lower.strip_suffix(&ending)
                && start.ends_with(char::is_whitespace)
                && let Some(kept) = company.get(..start.len())
            {
                return kept.trim_end();
            }
        }
    }
    company
}

/// Whether a register name is the post's company: most of the post's words in it, legal forms
/// aside ("Nordisk Fragt A/S" and "NORDISK FRAGT ApS").
fn same_company(wanted: &str, name: &str) -> bool {
    let want = name_words(wanted);
    let have = name_words(name);
    let shared = want.iter().filter(|word| have.contains(word)).count();
    !want.is_empty() && shared as f64 / want.len() as f64 >= SAME_NAME_SHARE
}

fn name_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2 && !NOT_NAME_WORDS.contains(word))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_the_same_company_whatever_its_legal_form() {
        assert!(same_company("Nordisk Fragt A/S", "NORDISK FRAGT ApS"));
        assert!(!same_company("Nordisk Fragt A/S", "Fragtmanden ApS"));
        assert!(!same_company("A/S", "Nordisk Fragt A/S"));
        assert_eq!(without_legal_form("Nordisk Fragt A/S"), "Nordisk Fragt");
        assert_eq!(without_legal_form("Firma ApS."), "Firma");
        assert_eq!(without_legal_form("Gaps"), "Gaps");
    }

    #[test]
    fn the_answer_is_taken_only_for_the_same_company() {
        let found = serde_json::json!({
            "name": "NORDISK FRAGT ApS",
            "address": "Havnevej 1",
            "zipcode": 8000,
            "city": "Aarhus C",
        });
        assert_eq!(
            address_of("Nordisk Fragt A/S", &found),
            Some(CvrAddress {
                name: "NORDISK FRAGT ApS".into(),
                address: "Havnevej 1".into(),
                postal_code_and_city: "8000 Aarhus C".into(),
            })
        );
        assert_eq!(address_of("Andet Firma", &found), None);
        assert_eq!(
            address_of("Nordisk Fragt", &serde_json::json!({"error": "NOT_FOUND"})),
            None
        );
    }
}
