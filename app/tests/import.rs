//! The import against a real MariaDB with db/schema.sql loaded. It needs one, so it is ignored
//! by default: RECRUITER_TEST_MYSQL=mysql://user:pass@127.0.0.1:3306/db cargo test -- --ignored
//! (the recruiter-desktop workflow runs it with a MariaDB service). The test empties that
//! database's cv_match tables, so never point it at real data.

use mysql::prelude::Queryable;
use recruiter::{db, import};

const ME: &str = "me@example.com";

/// Two candidates, each with a saved match: the import must bring only ME's rows.
const SEED: &str = r#"
DELETE FROM user_llm_keys; DELETE FROM candidate_resumes; DELETE FROM candidate_profile_links;
DELETE FROM candidate_profile_lines; DELETE FROM candidate_profile_entries; DELETE FROM candidate_profiles;
DELETE FROM cv_match_resume_lines; DELETE FROM cv_match_resume_entries; DELETE FROM cv_match_resumes;
DELETE FROM cv_match_rejections; DELETE FROM cv_match_integrations; DELETE FROM cv_match_items;
DELETE FROM cv_matches; DELETE FROM candidates; DELETE FROM trongate_users;
INSERT INTO trongate_users (id, code, user_level_id) VALUES (3, 'abc', 2), (4, 'def', 2);
INSERT INTO candidates (id, trongate_user_id, name, email, password, num_logins, phone, created_at, updated_at)
VALUES (11, 3, 'Me', 'me@example.com', '$2y$hash', 5, '12345678', 100, 200),
       (12, 4, 'Someone else', 'other@example.com', '$2y$hash', 1, NULL, 100, 200);
INSERT INTO cv_matches (id, trongate_user_id, job_title, company, job_text, cv_name, cv_text, score, points,
                        max_points, tag, application_text, created_at)
VALUES (21, 3, 'PHP-udvikler', 'Firma A/S', 'Vi søger en PHP-udvikler', 'cv.pdf', 'PHP, Laravel', 0.8140, 8, 10,
        'good', 'Kære Firma', 300),
       (22, 4, 'Mekaniker', '', 'post', 'cv', 'cv', 0.1000, 1, 10, 'poor', NULL, 300);
INSERT INTO cv_match_items (cv_match_id, criterion, text, verdict, decided_by)
VALUES (21, 'skills', 'PHP', 'met', 'exact'), (21, 'skills', 'Vue', 'missing', 'exact'),
       (22, 'skills', 'Motor', 'met', 'exact');
INSERT INTO cv_match_integrations (cv_match_id, integration, status, applied_at, created_at, updated_at)
VALUES (21, 'jobnet', 'Soegt', 400, 400, 400);
INSERT INTO cv_match_rejections (cv_match_id, reason, note, created_at) VALUES (22, 'location', 'far', 500);
INSERT INTO cv_match_resumes (cv_match_id, name, intro) VALUES (21, 'Me', 'Hej');
INSERT INTO cv_match_resume_entries (id, cv_match_id, section, title) VALUES (31, 21, 'experience', 'Developer');
INSERT INTO cv_match_resume_lines (cv_match_id, entry_id, kind, text) VALUES (21, 31, 'bullet', 'Built things'),
       (21, NULL, 'skill', 'PHP');
INSERT INTO candidate_profiles (candidate_id, about, created_at, updated_at) VALUES (11, 'About me', 100, 100),
       (12, 'About them', 100, 100);
INSERT INTO candidate_profile_entries (id, candidate_id, section, title, started_on, created_at, updated_at)
VALUES (41, 11, 'experience', 'Developer', '2017-01-01', 100, 100);
INSERT INTO candidate_profile_lines (candidate_id, entry_id, kind, text) VALUES (11, 41, 'bullet', 'Shipped');
INSERT INTO candidate_profile_links (candidate_id, label, url, created_at, updated_at)
VALUES (11, 'GitHub', 'https://github.com/example', 100, 100);
INSERT INTO candidate_resumes (candidate_id, cv_text, confirmed_at, created_at, updated_at)
VALUES (11, 'PHP, Laravel', 100, 100, 100);
INSERT INTO user_llm_keys (trongate_user_id, provider, api_key_encrypted, key_hint, created_at, updated_at)
VALUES (3, 'openai', 'encrypted', 'abcd', 100, 100);
"#;

fn source() -> String {
    std::env::var("RECRUITER_TEST_MYSQL").expect("RECRUITER_TEST_MYSQL names the test MariaDB")
}

fn seed(url: &str) {
    let mut conn = mysql::Conn::new(mysql::Opts::from_url(url).unwrap()).unwrap();
    conn.query_drop(SEED).unwrap();
}

fn count(target: &rusqlite::Connection, table: &str) -> i64 {
    target
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
        .unwrap()
}

#[test]
#[ignore = "needs a MariaDB with db/schema.sql: set RECRUITER_TEST_MYSQL"]
fn copies_only_my_rows_with_their_ids() {
    let url = source();
    seed(&url);
    let mut target = db::open_in_memory().unwrap();
    let report = import::import(&url, ME, &mut target).unwrap();

    assert!(report.skipped_llm_key);
    assert_eq!(count(&target, "candidates"), 1);
    assert_eq!(count(&target, "cv_matches"), 1);
    assert_eq!(count(&target, "cv_match_items"), 2);
    assert_eq!(count(&target, "cv_match_integrations"), 1);
    assert_eq!(count(&target, "cv_match_rejections"), 0);
    assert_eq!(count(&target, "cv_match_resume_lines"), 2);
    assert_eq!(count(&target, "candidate_profiles"), 1);
    assert_eq!(count(&target, "candidate_profile_links"), 1);
    assert_eq!(count(&target, "user_llm_keys"), 0);

    let (id, score, title): (i64, f64, String) = target
        .query_row("SELECT id, score, job_title FROM cv_matches", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!((id, score, title.as_str()), (21, 0.814, "PHP-udvikler"));
    let started: String = target
        .query_row("SELECT started_on FROM candidate_profile_entries", [], |row| row.get(0))
        .unwrap();
    assert_eq!(started, "2017-01-01");
    let entry: i64 = target
        .query_row(
            "SELECT entry_id FROM cv_match_resume_lines WHERE kind = 'bullet'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(entry, 31);
}

#[test]
#[ignore = "needs a MariaDB with db/schema.sql: set RECRUITER_TEST_MYSQL"]
fn refuses_a_second_import_and_an_unknown_email() {
    let url = source();
    seed(&url);
    let mut target = db::open_in_memory().unwrap();
    assert!(matches!(
        import::import(&url, "nobody@example.com", &mut target),
        Err(import::Error::NoSuchUser(_))
    ));
    assert_eq!(count(&target, "trongate_users"), 0);
    import::import(&url, ME, &mut target).unwrap();
    assert!(matches!(
        import::import(&url, ME, &mut target),
        Err(import::Error::AlreadyImported)
    ));
    assert_eq!(count(&target, "cv_matches"), 1);
}
