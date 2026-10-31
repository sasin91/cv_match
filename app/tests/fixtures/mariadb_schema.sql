-- The web app's MariaDB schema (recruiter's db/schema.sql at its last version, comments left
-- out): the tables recruiter import reads, loaded into a scratch MariaDB by tests/import.rs.


CREATE TABLE IF NOT EXISTS `trongate_user_levels` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `level_title` varchar(125) DEFAULT '',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

INSERT IGNORE INTO `trongate_user_levels` (`id`, `level_title`) VALUES
(1, 'admin'),
(2, 'company_member'),
(3, 'candidate');

CREATE TABLE IF NOT EXISTS `trongate_users` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `code` varchar(32) DEFAULT NULL,
  `user_level_id` int(11) DEFAULT 0,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `trongate_administrators` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `username` varchar(65) DEFAULT NULL,
  `email` varchar(255) DEFAULT NULL,
  `password` varchar(60) DEFAULT NULL,
  `trongate_user_id` int(11) DEFAULT NULL,
  `active` tinyint(1) NOT NULL DEFAULT 0,
  `failed_login_attempts` int(11) NOT NULL DEFAULT 0,
  `last_failed_attempt` int(11) NOT NULL DEFAULT 0,
  `login_blocked_until` int(11) NOT NULL DEFAULT 0,
  `failed_login_ip` varchar(45) NOT NULL DEFAULT '',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `trongate_tokens` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `token` varchar(125) DEFAULT NULL,
  `user_id` int(11) DEFAULT 0,
  `expiry_date` int(11) DEFAULT NULL,
  `code` varchar(3) DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `login_attempts` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `target_table` varchar(255) NOT NULL,
  `identifier` varchar(255) NOT NULL,
  `ip_address` varchar(45) NOT NULL DEFAULT '',
  `attempted_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `target_table` (`target_table`(191)),
  KEY `identifier` (`identifier`(191))
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `password_resets` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `target_table` varchar(255) NOT NULL,
  `identifier` varchar(255) NOT NULL,
  `token` varchar(255) NOT NULL,
  `expiry_date` int(11) NOT NULL,
  `used` tinyint(1) NOT NULL DEFAULT 0,
  `created_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `token` (`token`(191))
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `terms` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `kind` varchar(24) NOT NULL,
  `parent_id` int(11) DEFAULT NULL,
  `slug` varchar(191) NOT NULL,
  `definition` text DEFAULT NULL,
  `source` varchar(24) NOT NULL DEFAULT 'manual',
  `status` varchar(16) NOT NULL DEFAULT 'active',
  `merged_into_id` int(11) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `kind_slug` (`kind`, `slug`),
  KEY `parent_id` (`parent_id`),
  KEY `merged_into_id` (`merged_into_id`),
  CONSTRAINT `terms_parent_fk` FOREIGN KEY (`parent_id`) REFERENCES `terms` (`id`) ON DELETE SET NULL,
  CONSTRAINT `terms_merged_into_fk` FOREIGN KEY (`merged_into_id`) REFERENCES `terms` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `term_labels` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `term_id` int(11) NOT NULL,
  `lang` char(2) NOT NULL,
  `label` varchar(191) NOT NULL,
  `normalised` varchar(191) COLLATE utf8mb4_bin NOT NULL,
  `is_preferred` tinyint(1) NOT NULL DEFAULT 0,
  PRIMARY KEY (`id`),
  UNIQUE KEY `term_lang_normalised` (`term_id`, `lang`, `normalised`),
  KEY `normalised_lang` (`normalised`, `lang`),
  CONSTRAINT `term_labels_term_fk` FOREIGN KEY (`term_id`) REFERENCES `terms` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `term_relations` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `term_id` int(11) NOT NULL,
  `related_term_id` int(11) NOT NULL,
  `relation` varchar(24) NOT NULL,
  `confidence` decimal(4,3) DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `term_related_relation` (`term_id`, `related_term_id`, `relation`),
  KEY `related_term_id` (`related_term_id`),
  CONSTRAINT `term_relations_term_fk` FOREIGN KEY (`term_id`) REFERENCES `terms` (`id`) ON DELETE CASCADE,
  CONSTRAINT `term_relations_related_fk` FOREIGN KEY (`related_term_id`) REFERENCES `terms` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `companies` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `name` varchar(255) NOT NULL,
  `cvr_number` varchar(8) DEFAULT NULL,
  `company_type_term_id` int(11) DEFAULT NULL,
  `contact_email` varchar(255) DEFAULT NULL,
  `active` tinyint(1) NOT NULL DEFAULT 1,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `cvr_number` (`cvr_number`),
  KEY `company_type_term_id` (`company_type_term_id`),
  CONSTRAINT `companies_company_type_fk` FOREIGN KEY (`company_type_term_id`) REFERENCES `terms` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `companies`
  ADD COLUMN IF NOT EXISTS `contact_email` varchar(255) DEFAULT NULL AFTER `company_type_term_id`,
  ADD COLUMN IF NOT EXISTS `active` tinyint(1) NOT NULL DEFAULT 1 AFTER `contact_email`;

CREATE TABLE IF NOT EXISTS `company_members` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `company_id` int(11) NOT NULL,
  `trongate_user_id` int(11) NOT NULL,
  `name` varchar(255) NOT NULL,
  `email` varchar(255) NOT NULL,
  `password` varchar(60) DEFAULT NULL,
  `role` varchar(16) NOT NULL DEFAULT 'recruiter',
  `active` tinyint(1) NOT NULL DEFAULT 1,
  `invite_token` char(32) DEFAULT NULL,
  `invited_at` int(11) DEFAULT NULL,
  `num_logins` int(11) NOT NULL DEFAULT 0,
  `last_login` int(11) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `trongate_user_id` (`trongate_user_id`),
  UNIQUE KEY `email` (`email`),
  UNIQUE KEY `invite_token` (`invite_token`),
  KEY `company_id` (`company_id`),
  CONSTRAINT `company_members_company_fk` FOREIGN KEY (`company_id`) REFERENCES `companies` (`id`) ON DELETE CASCADE,
  CONSTRAINT `company_members_user_fk` FOREIGN KEY (`trongate_user_id`) REFERENCES `trongate_users` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `company_members`
  ADD COLUMN IF NOT EXISTS `invite_token` char(32) DEFAULT NULL AFTER `active`,
  ADD COLUMN IF NOT EXISTS `invited_at` int(11) DEFAULT NULL AFTER `invite_token`,
  ADD UNIQUE KEY IF NOT EXISTS `invite_token` (`invite_token`);

CREATE TABLE IF NOT EXISTS `candidates` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `trongate_user_id` int(11) NOT NULL,
  `name` varchar(255) NOT NULL,
  `email` varchar(255) NOT NULL,
  `password` varchar(60) DEFAULT NULL,
  `active` tinyint(1) NOT NULL DEFAULT 1,
  `num_logins` int(11) NOT NULL DEFAULT 0,
  `last_login` int(11) DEFAULT NULL,
  `phone` varchar(32) DEFAULT NULL,
  `postal_code` varchar(10) DEFAULT NULL,
  `open_to_work` tinyint(1) NOT NULL DEFAULT 0,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `trongate_user_id` (`trongate_user_id`),
  UNIQUE KEY `email` (`email`),
  CONSTRAINT `candidates_user_fk` FOREIGN KEY (`trongate_user_id`) REFERENCES `trongate_users` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `candidates`
  ADD COLUMN IF NOT EXISTS `open_to_work` tinyint(1) NOT NULL DEFAULT 0 AFTER `postal_code`;

CREATE TABLE IF NOT EXISTS `candidate_profile_links` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `candidate_id` int(11) NOT NULL,
  `label` varchar(64) NOT NULL,
  `url` varchar(512) NOT NULL,
  `position` tinyint(3) unsigned NOT NULL DEFAULT 0,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `candidate_label` (`candidate_id`, `label`),
  CONSTRAINT `candidate_profile_links_candidate_fk` FOREIGN KEY (`candidate_id`) REFERENCES `candidates` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `candidate_profiles` (
  `candidate_id` int(11) NOT NULL,
  `title` varchar(255) NOT NULL DEFAULT '',
  `location` varchar(255) NOT NULL DEFAULT '',
  `about` text NOT NULL,
  `filled_from_cv_name` varchar(255) NOT NULL DEFAULT '',
  `filled_from_cv_at` int(11) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`candidate_id`),
  CONSTRAINT `candidate_profiles_candidate_fk` FOREIGN KEY (`candidate_id`) REFERENCES `candidates` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `candidate_profile_entries` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `candidate_id` int(11) NOT NULL,
  `section` varchar(12) NOT NULL,
  `title` varchar(255) NOT NULL,
  `organisation` varchar(255) NOT NULL DEFAULT '',
  `location` varchar(255) NOT NULL DEFAULT '',
  `url` varchar(512) NOT NULL DEFAULT '',
  `started_on` date DEFAULT NULL,
  `ended_on` date DEFAULT NULL,
  `summary` varchar(500) NOT NULL DEFAULT '',
  `note` varchar(1000) NOT NULL DEFAULT '',
  `sort_order` smallint(6) NOT NULL DEFAULT 0,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `candidate_section` (`candidate_id`, `section`, `sort_order`),
  CONSTRAINT `candidate_profile_entries_candidate_fk` FOREIGN KEY (`candidate_id`) REFERENCES `candidates` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `candidate_profile_lines` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `candidate_id` int(11) NOT NULL,
  `entry_id` int(11) DEFAULT NULL,
  `kind` varchar(12) NOT NULL,
  `text` varchar(1000) NOT NULL,
  `sort_order` smallint(6) NOT NULL DEFAULT 0,
  PRIMARY KEY (`id`),
  KEY `candidate_kind` (`candidate_id`, `kind`, `sort_order`),
  KEY `entry_id` (`entry_id`, `sort_order`),
  CONSTRAINT `candidate_profile_lines_candidate_fk` FOREIGN KEY (`candidate_id`) REFERENCES `candidates` (`id`) ON DELETE CASCADE,
  CONSTRAINT `candidate_profile_lines_entry_fk` FOREIGN KEY (`entry_id`) REFERENCES `candidate_profile_entries` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `job_posts` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `company_id` int(11) NOT NULL,
  `created_by` int(11) DEFAULT NULL,
  `title` varchar(255) NOT NULL,
  `level` varchar(24) DEFAULT NULL,
  `workplace_flexibility` varchar(16) DEFAULT NULL,
  `work_hours` varchar(16) DEFAULT NULL,
  `postal_code` varchar(10) DEFAULT NULL,
  `country_code` char(2) NOT NULL DEFAULT 'DK',
  `language` char(2) NOT NULL DEFAULT 'da',
  `status` varchar(16) NOT NULL DEFAULT 'draft',
  `version` int(11) NOT NULL DEFAULT 1,
  `raw_text` mediumtext NOT NULL,
  `pitch` mediumtext DEFAULT NULL,
  `extractor` varchar(32) NOT NULL DEFAULT 'manual',
  `external_id` varchar(64) DEFAULT NULL,
  `public_token` char(12) DEFAULT NULL,
  `accepts_suggested` tinyint(1) NOT NULL DEFAULT 1,
  `max_suggested_applications` smallint(6) NOT NULL DEFAULT 20,
  `published_at` int(11) DEFAULT NULL,
  `paused_at` int(11) DEFAULT NULL,
  `closed_at` int(11) DEFAULT NULL,
  `archived_at` int(11) DEFAULT NULL,
  `closes_at` int(11) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `public_token` (`public_token`),
  KEY `company_status` (`company_id`, `status`),
  KEY `status_published` (`status`, `published_at`),
  KEY `created_by` (`created_by`),
  CONSTRAINT `job_posts_company_fk` FOREIGN KEY (`company_id`) REFERENCES `companies` (`id`),
  CONSTRAINT `job_posts_created_by_fk` FOREIGN KEY (`created_by`) REFERENCES `company_members` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `job_posts`
  ADD COLUMN IF NOT EXISTS `pitch` mediumtext DEFAULT NULL AFTER `raw_text`,
  ADD COLUMN IF NOT EXISTS `public_token` char(12) DEFAULT NULL AFTER `external_id`,
  ADD COLUMN IF NOT EXISTS `accepts_suggested` tinyint(1) NOT NULL DEFAULT 1 AFTER `public_token`,
  ADD COLUMN IF NOT EXISTS `max_suggested_applications` smallint(6) NOT NULL DEFAULT 20 AFTER `accepts_suggested`,
  ADD COLUMN IF NOT EXISTS `paused_at` int(11) DEFAULT NULL AFTER `published_at`,
  ADD COLUMN IF NOT EXISTS `closed_at` int(11) DEFAULT NULL AFTER `paused_at`,
  ADD COLUMN IF NOT EXISTS `archived_at` int(11) DEFAULT NULL AFTER `closed_at`,
  ADD UNIQUE KEY IF NOT EXISTS `public_token` (`public_token`),
  DROP COLUMN IF EXISTS `min_experience_years`,
  DROP COLUMN IF EXISTS `education_level`;

CREATE TABLE IF NOT EXISTS `job_post_terms` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `job_post_id` int(11) NOT NULL,
  `kind` varchar(24) NOT NULL,
  `term_id` int(11) DEFAULT NULL,
  `raw_text` varchar(255) NOT NULL,
  `english` varchar(255) DEFAULT NULL,
  `normalised` varchar(191) COLLATE utf8mb4_bin NOT NULL,
  `is_required` tinyint(1) NOT NULL DEFAULT 0,
  `is_highlighted` tinyint(1) NOT NULL DEFAULT 0,
  `alt_group` smallint(6) DEFAULT NULL,
  `min_years` tinyint(3) unsigned DEFAULT NULL,
  `min_level` varchar(24) DEFAULT NULL,
  `match_method` varchar(16) NOT NULL DEFAULT 'none',
  `similarity` decimal(4,3) DEFAULT NULL,
  `sort_order` smallint(6) NOT NULL DEFAULT 0,
  PRIMARY KEY (`id`),
  KEY `job_post_kind` (`job_post_id`, `kind`),
  KEY `job_post_alt_group` (`job_post_id`, `alt_group`),
  KEY `term_id` (`term_id`),
  KEY `kind_normalised` (`kind`, `normalised`),
  CONSTRAINT `job_post_terms_job_post_fk` FOREIGN KEY (`job_post_id`) REFERENCES `job_posts` (`id`) ON DELETE CASCADE,
  CONSTRAINT `job_post_terms_term_fk` FOREIGN KEY (`term_id`) REFERENCES `terms` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `job_post_terms`
  ADD COLUMN IF NOT EXISTS `english` varchar(255) DEFAULT NULL AFTER `raw_text`,
  ADD COLUMN IF NOT EXISTS `alt_group` smallint(6) DEFAULT NULL AFTER `is_highlighted`,
  ADD COLUMN IF NOT EXISTS `min_years` tinyint(3) unsigned DEFAULT NULL AFTER `alt_group`,
  ADD COLUMN IF NOT EXISTS `min_level` varchar(24) DEFAULT NULL AFTER `min_years`,
  ADD KEY IF NOT EXISTS `job_post_alt_group` (`job_post_id`, `alt_group`);

CREATE TABLE IF NOT EXISTS `job_post_member_views` (
  `job_post_id` int(11) NOT NULL,
  `company_member_id` int(11) NOT NULL,
  `last_viewed_at` int(11) NOT NULL,
  PRIMARY KEY (`job_post_id`, `company_member_id`),
  KEY `company_member_id` (`company_member_id`),
  CONSTRAINT `job_post_member_views_post_fk` FOREIGN KEY (`job_post_id`) REFERENCES `job_posts` (`id`) ON DELETE CASCADE,
  CONSTRAINT `job_post_member_views_member_fk` FOREIGN KEY (`company_member_id`) REFERENCES `company_members` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `job_applications` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `job_post_id` int(11) NOT NULL,
  `candidate_id` int(11) NOT NULL,
  `current_title` varchar(255) DEFAULT NULL,
  `level` varchar(24) DEFAULT NULL,
  `workplace_flexibility` varchar(16) DEFAULT NULL,
  `work_hours` varchar(16) DEFAULT NULL,
  `postal_code` varchar(10) DEFAULT NULL,
  `language` char(2) NOT NULL DEFAULT 'da',
  `source` varchar(16) NOT NULL DEFAULT 'form',
  `raw_text` mediumtext DEFAULT NULL,
  `cover_letter` mediumtext DEFAULT NULL,
  `extractor` varchar(32) NOT NULL DEFAULT 'manual',
  `candidate_resume_version` int(11) DEFAULT NULL,
  `status` varchar(16) NOT NULL DEFAULT 'draft',
  `submitted_at` int(11) DEFAULT NULL,
  `shortlisted_at` int(11) DEFAULT NULL,
  `bookmarked_at` int(11) DEFAULT NULL,
  `rejected_at` int(11) DEFAULT NULL,
  `reject_reason` varchar(255) DEFAULT NULL,
  `withdrawn_at` int(11) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `job_post_candidate` (`job_post_id`, `candidate_id`),
  KEY `job_post_status` (`job_post_id`, `status`),
  KEY `candidate_id` (`candidate_id`),
  CONSTRAINT `job_applications_job_post_fk` FOREIGN KEY (`job_post_id`) REFERENCES `job_posts` (`id`),
  CONSTRAINT `job_applications_candidate_fk` FOREIGN KEY (`candidate_id`) REFERENCES `candidates` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `job_application_terms` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `job_application_id` int(11) NOT NULL,
  `kind` varchar(24) NOT NULL,
  `term_id` int(11) DEFAULT NULL,
  `raw_text` varchar(255) NOT NULL,
  `normalised` varchar(191) COLLATE utf8mb4_bin NOT NULL,
  `years` tinyint(3) unsigned DEFAULT NULL,
  `level` varchar(24) DEFAULT NULL,
  `is_highlighted` tinyint(1) NOT NULL DEFAULT 0,
  `match_method` varchar(16) NOT NULL DEFAULT 'none',
  `similarity` decimal(4,3) DEFAULT NULL,
  `sort_order` smallint(6) NOT NULL DEFAULT 0,
  PRIMARY KEY (`id`),
  KEY `job_application_kind` (`job_application_id`, `kind`),
  KEY `term_id` (`term_id`),
  KEY `kind_normalised` (`kind`, `normalised`),
  CONSTRAINT `job_application_terms_application_fk` FOREIGN KEY (`job_application_id`) REFERENCES `job_applications` (`id`) ON DELETE CASCADE,
  CONSTRAINT `job_application_terms_term_fk` FOREIGN KEY (`term_id`) REFERENCES `terms` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `job_applications`
  ADD COLUMN IF NOT EXISTS `cover_letter` mediumtext DEFAULT NULL AFTER `raw_text`,
  ADD COLUMN IF NOT EXISTS `candidate_resume_version` int(11) DEFAULT NULL AFTER `extractor`,
  ADD COLUMN IF NOT EXISTS `bookmarked_at` int(11) DEFAULT NULL AFTER `shortlisted_at`,
  ADD COLUMN IF NOT EXISTS `withdrawn_at` int(11) DEFAULT NULL AFTER `reject_reason`,
  DROP COLUMN IF EXISTS `experience_years`,
  DROP COLUMN IF EXISTS `education_level`;
ALTER TABLE `job_application_terms`
  ADD COLUMN IF NOT EXISTS `level` varchar(24) DEFAULT NULL AFTER `years`;

CREATE TABLE IF NOT EXISTS `candidate_resumes` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `candidate_id` int(11) NOT NULL,
  `version` int(11) NOT NULL DEFAULT 1,
  `cv_name` varchar(255) NOT NULL DEFAULT '',
  `cv_text` mediumtext NOT NULL,
  `current_title` varchar(255) DEFAULT NULL,
  `postal_code` varchar(10) DEFAULT NULL,
  `experience_years` tinyint(3) unsigned DEFAULT NULL,
  `language` char(2) NOT NULL DEFAULT 'da',
  `extractor` varchar(32) NOT NULL DEFAULT 'manual',
  `confirmed_at` int(11) NOT NULL,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `candidate_id` (`candidate_id`),
  CONSTRAINT `candidate_resumes_candidate_fk` FOREIGN KEY (`candidate_id`) REFERENCES `candidates` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `candidate_resume_terms` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `candidate_resume_id` int(11) NOT NULL,
  `kind` varchar(24) NOT NULL,
  `term_id` int(11) DEFAULT NULL,
  `raw_text` varchar(255) NOT NULL,
  `normalised` varchar(191) COLLATE utf8mb4_bin NOT NULL,
  `years` tinyint(3) unsigned DEFAULT NULL,
  `level` varchar(24) DEFAULT NULL,
  `match_method` varchar(16) NOT NULL DEFAULT 'none',
  `similarity` decimal(4,3) DEFAULT NULL,
  `sort_order` smallint(6) NOT NULL DEFAULT 0,
  PRIMARY KEY (`id`),
  KEY `candidate_resume_kind` (`candidate_resume_id`, `kind`),
  KEY `term_id` (`term_id`),
  KEY `kind_normalised` (`kind`, `normalised`),
  CONSTRAINT `candidate_resume_terms_resume_fk` FOREIGN KEY (`candidate_resume_id`) REFERENCES `candidate_resumes` (`id`) ON DELETE CASCADE,
  CONSTRAINT `candidate_resume_terms_term_fk` FOREIGN KEY (`term_id`) REFERENCES `terms` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `attributes` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `owner_type` varchar(16) NOT NULL,
  `owner_id` int(11) NOT NULL,
  `attr_key` varchar(64) NOT NULL,
  `value` text NOT NULL,
  `lang` char(2) DEFAULT NULL,
  `sort_order` smallint(6) NOT NULL DEFAULT 0,
  `created_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `owner` (`owner_type`, `owner_id`),
  KEY `attr_key` (`attr_key`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `unmatched_terms` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `kind` varchar(24) NOT NULL,
  `lang` char(2) NOT NULL,
  `normalised` varchar(191) COLLATE utf8mb4_bin NOT NULL,
  `example_raw_text` varchar(255) NOT NULL,
  `occurrences` int(11) NOT NULL DEFAULT 1,
  `suggested_term_id` int(11) DEFAULT NULL,
  `suggested_similarity` decimal(4,3) DEFAULT NULL,
  `status` varchar(16) NOT NULL DEFAULT 'pending',
  `resolved_term_id` int(11) DEFAULT NULL,
  `resolved_by` int(11) DEFAULT NULL,
  `resolved_at` int(11) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `kind_lang_normalised` (`kind`, `lang`, `normalised`),
  KEY `status` (`status`),
  KEY `suggested_term_id` (`suggested_term_id`),
  KEY `resolved_term_id` (`resolved_term_id`),
  KEY `resolved_by` (`resolved_by`),
  CONSTRAINT `unmatched_terms_suggested_fk` FOREIGN KEY (`suggested_term_id`) REFERENCES `terms` (`id`) ON DELETE SET NULL,
  CONSTRAINT `unmatched_terms_resolved_fk` FOREIGN KEY (`resolved_term_id`) REFERENCES `terms` (`id`) ON DELETE SET NULL,
  CONSTRAINT `unmatched_terms_resolved_by_fk` FOREIGN KEY (`resolved_by`) REFERENCES `trongate_users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `match_scores` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `job_post_id` int(11) NOT NULL,
  `job_application_id` int(11) NOT NULL,
  `job_post_version` int(11) NOT NULL,
  `ranking_version` varchar(16) NOT NULL,
  `deterministic_score` decimal(5,4) NOT NULL,
  `llm_score` decimal(5,4) DEFAULT NULL,
  `combined_score` decimal(5,4) NOT NULL,
  `tag` varchar(8) NOT NULL,
  `laya_requirements_probability` decimal(5,4) DEFAULT NULL,
  `laya_fit_expected` decimal(4,3) DEFAULT NULL,
  `laya_choice` varchar(16) DEFAULT NULL,
  `needs_human` tinyint(1) NOT NULL DEFAULT 0,
  `final_rank` smallint(6) DEFAULT NULL,
  `computed_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `application_versions` (`job_application_id`, `job_post_version`, `ranking_version`),
  KEY `job_post_combined` (`job_post_id`, `combined_score`),
  KEY `job_post_rank` (`job_post_id`, `final_rank`),
  CONSTRAINT `match_scores_job_post_fk` FOREIGN KEY (`job_post_id`) REFERENCES `job_posts` (`id`) ON DELETE CASCADE,
  CONSTRAINT `match_scores_application_fk` FOREIGN KEY (`job_application_id`) REFERENCES `job_applications` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `match_score_details` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `match_score_id` int(11) NOT NULL,
  `stage` varchar(16) NOT NULL,
  `criterion` varchar(32) NOT NULL,
  `job_post_term_id` int(11) DEFAULT NULL,
  `job_application_term_id` int(11) DEFAULT NULL,
  `rule` varchar(16) NOT NULL,
  `weight` decimal(6,2) NOT NULL DEFAULT 0.00,
  `rank` decimal(5,4) NOT NULL DEFAULT 0.0000,
  `passed` tinyint(1) NOT NULL DEFAULT 0,
  `probability` decimal(5,4) DEFAULT NULL,
  `reason` varchar(500) DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `match_score_stage` (`match_score_id`, `stage`),
  KEY `job_post_term_id` (`job_post_term_id`),
  KEY `job_application_term_id` (`job_application_term_id`),
  CONSTRAINT `match_score_details_score_fk` FOREIGN KEY (`match_score_id`) REFERENCES `match_scores` (`id`) ON DELETE CASCADE,
  CONSTRAINT `match_score_details_job_post_term_fk` FOREIGN KEY (`job_post_term_id`) REFERENCES `job_post_terms` (`id`) ON DELETE SET NULL,
  CONSTRAINT `match_score_details_application_term_fk` FOREIGN KEY (`job_application_term_id`) REFERENCES `job_application_terms` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `job_application_actions` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `job_application_id` int(11) NOT NULL,
  `company_member_id` int(11) DEFAULT NULL,
  `candidate_id` int(11) DEFAULT NULL,
  `action` varchar(24) NOT NULL,
  `from_status` varchar(16) DEFAULT NULL,
  `to_status` varchar(16) NOT NULL,
  `job_post_version` int(11) NOT NULL,
  `match_score_id` int(11) DEFAULT NULL,
  `final_rank` smallint(6) DEFAULT NULL,
  `source` varchar(24) NOT NULL,
  `reason` varchar(32) DEFAULT NULL,
  `note` varchar(500) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `application_created` (`job_application_id`, `created_at`),
  KEY `company_member_id` (`company_member_id`),
  KEY `candidate_id` (`candidate_id`),
  KEY `match_score_id` (`match_score_id`),
  KEY `action_reason` (`action`, `reason`),
  CONSTRAINT `job_application_actions_application_fk` FOREIGN KEY (`job_application_id`) REFERENCES `job_applications` (`id`) ON DELETE CASCADE,
  CONSTRAINT `job_application_actions_member_fk` FOREIGN KEY (`company_member_id`) REFERENCES `company_members` (`id`) ON DELETE SET NULL,
  CONSTRAINT `job_application_actions_candidate_fk` FOREIGN KEY (`candidate_id`) REFERENCES `candidates` (`id`) ON DELETE SET NULL,
  CONSTRAINT `job_application_actions_score_fk` FOREIGN KEY (`match_score_id`) REFERENCES `match_scores` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `job_application_actions`
  ADD COLUMN IF NOT EXISTS `from_status` varchar(16) DEFAULT NULL AFTER `action`,
  ADD COLUMN IF NOT EXISTS `to_status` varchar(16) NOT NULL DEFAULT '' AFTER `from_status`,
  ADD COLUMN IF NOT EXISTS `job_post_version` int(11) NOT NULL DEFAULT 0 AFTER `to_status`,
  ADD COLUMN IF NOT EXISTS `match_score_id` int(11) DEFAULT NULL AFTER `job_post_version`,
  ADD COLUMN IF NOT EXISTS `final_rank` smallint(6) DEFAULT NULL AFTER `match_score_id`,
  ADD COLUMN IF NOT EXISTS `source` varchar(24) NOT NULL DEFAULT '' AFTER `final_rank`,
  ADD COLUMN IF NOT EXISTS `reason` varchar(32) DEFAULT NULL AFTER `source`,
  ADD KEY IF NOT EXISTS `match_score_id` (`match_score_id`),
  ADD KEY IF NOT EXISTS `action_reason` (`action`, `reason`),
  ADD CONSTRAINT `job_application_actions_score_fk` FOREIGN KEY IF NOT EXISTS (`match_score_id`) REFERENCES `match_scores` (`id`) ON DELETE SET NULL;

CREATE TABLE IF NOT EXISTS `cv_matches` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `trongate_user_id` int(11) DEFAULT NULL,
  `job_title` varchar(255) NOT NULL,
  `company` varchar(255) NOT NULL DEFAULT '',
  `job_url` varchar(2048) DEFAULT NULL,
  `job_text` mediumtext NOT NULL,
  `cv_name` varchar(255) NOT NULL DEFAULT '',
  `cv_text` mediumtext NOT NULL,
  `score` decimal(5,4) NOT NULL,
  `points` smallint(6) NOT NULL,
  `max_points` smallint(6) NOT NULL,
  `tag` varchar(8) NOT NULL,
  `application_text` mediumtext DEFAULT NULL,
  `application_written_at` int(11) DEFAULT NULL,
  `resume_text` mediumtext DEFAULT NULL,
  `resume_written_at` int(11) DEFAULT NULL,
  `rejected_at` int(11) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `user_created` (`trongate_user_id`, `created_at`),
  CONSTRAINT `cv_matches_user_fk` FOREIGN KEY (`trongate_user_id`) REFERENCES `trongate_users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `cv_matches`
  ADD COLUMN IF NOT EXISTS `resume_text` mediumtext DEFAULT NULL AFTER `application_written_at`,
  ADD COLUMN IF NOT EXISTS `resume_written_at` int(11) DEFAULT NULL AFTER `resume_text`;

ALTER TABLE `cv_matches`
  ADD COLUMN IF NOT EXISTS `rejected_at` int(11) DEFAULT NULL AFTER `resume_written_at`;

CREATE TABLE IF NOT EXISTS `cv_match_items` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `cv_match_id` int(11) NOT NULL,
  `criterion` varchar(24) NOT NULL,
  `text` varchar(500) NOT NULL,
  `verdict` varchar(8) NOT NULL,
  `decided_by` varchar(16) NOT NULL,
  `reason` varchar(500) NOT NULL DEFAULT '',
  `evidence` varchar(500) NOT NULL DEFAULT '',
  `sort_order` smallint(6) NOT NULL DEFAULT 0,
  PRIMARY KEY (`id`),
  KEY `cv_match_id` (`cv_match_id`, `sort_order`),
  CONSTRAINT `cv_match_items_match_fk` FOREIGN KEY (`cv_match_id`) REFERENCES `cv_matches` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `cv_match_resumes` (
  `cv_match_id` int(11) NOT NULL,
  `name` varchar(255) NOT NULL,
  `title` varchar(255) NOT NULL DEFAULT '',
  `location` varchar(255) NOT NULL DEFAULT '',
  `phone` varchar(64) NOT NULL DEFAULT '',
  `email` varchar(255) NOT NULL DEFAULT '',
  `links` varchar(1000) NOT NULL DEFAULT '',
  `intro` text NOT NULL,
  `education_note` varchar(500) NOT NULL DEFAULT '',
  `experience_heading` varchar(64) NOT NULL DEFAULT 'Experience',
  `skills_heading` varchar(64) NOT NULL DEFAULT 'Skills',
  `education_heading` varchar(64) NOT NULL DEFAULT 'Education',
  `languages_heading` varchar(64) NOT NULL DEFAULT 'Languages',
  `projects_heading` varchar(64) NOT NULL DEFAULT 'Projects',
  PRIMARY KEY (`cv_match_id`),
  CONSTRAINT `cv_match_resumes_match_fk` FOREIGN KEY (`cv_match_id`) REFERENCES `cv_matches` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `cv_match_resumes`
  ADD COLUMN IF NOT EXISTS `projects_heading` varchar(64) NOT NULL DEFAULT 'Projects' AFTER `languages_heading`;

CREATE TABLE IF NOT EXISTS `cv_match_resume_entries` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `cv_match_id` int(11) NOT NULL,
  `section` varchar(12) NOT NULL,
  `title` varchar(255) NOT NULL,
  `organisation` varchar(255) NOT NULL DEFAULT '',
  `location` varchar(255) NOT NULL DEFAULT '',
  `starts` varchar(32) NOT NULL DEFAULT '',
  `ends` varchar(32) NOT NULL DEFAULT '',
  `summary` varchar(500) NOT NULL DEFAULT '',
  `note` varchar(1000) NOT NULL DEFAULT '',
  `sort_order` smallint(6) NOT NULL DEFAULT 0,
  PRIMARY KEY (`id`),
  KEY `cv_match_id` (`cv_match_id`, `section`, `sort_order`),
  CONSTRAINT `cv_match_resume_entries_match_fk` FOREIGN KEY (`cv_match_id`) REFERENCES `cv_matches` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `cv_match_resume_lines` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `cv_match_id` int(11) NOT NULL,
  `entry_id` int(11) DEFAULT NULL,
  `kind` varchar(12) NOT NULL,
  `text` varchar(1000) NOT NULL,
  `sort_order` smallint(6) NOT NULL DEFAULT 0,
  PRIMARY KEY (`id`),
  KEY `cv_match_id` (`cv_match_id`, `kind`, `sort_order`),
  KEY `entry_id` (`entry_id`, `sort_order`),
  CONSTRAINT `cv_match_resume_lines_match_fk` FOREIGN KEY (`cv_match_id`) REFERENCES `cv_matches` (`id`) ON DELETE CASCADE,
  CONSTRAINT `cv_match_resume_lines_entry_fk` FOREIGN KEY (`entry_id`) REFERENCES `cv_match_resume_entries` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `cv_match_integrations` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `cv_match_id` int(11) NOT NULL,
  `integration` varchar(32) NOT NULL,
  `status` varchar(32) NOT NULL,
  `external_id` varchar(255) DEFAULT NULL,
  `logged_at` int(11) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `match_integration` (`cv_match_id`, `integration`),
  CONSTRAINT `cv_match_integrations_match_fk` FOREIGN KEY (`cv_match_id`) REFERENCES `cv_matches` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `cv_match_integrations`
  ADD COLUMN IF NOT EXISTS `applied_at` int(11) DEFAULT NULL AFTER `external_id`;

CREATE TABLE IF NOT EXISTS `cv_match_rejections` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `cv_match_id` int(11) NOT NULL,
  `reason` varchar(16) NOT NULL,
  `note` varchar(255) NOT NULL DEFAULT '',
  `created_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `match_reason` (`cv_match_id`, `reason`),
  KEY `reason` (`reason`),
  CONSTRAINT `cv_match_rejections_match_fk` FOREIGN KEY (`cv_match_id`) REFERENCES `cv_matches` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `job_searches` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `trongate_user_id` int(11) NOT NULL,
  `name` varchar(120) NOT NULL,
  `keywords` varchar(500) NOT NULL,
  `location` varchar(255) NOT NULL DEFAULT '',
  `remote_ok` tinyint(1) NOT NULL DEFAULT 1,
  `cv_name` varchar(255) NOT NULL DEFAULT '',
  `cv_text` mediumtext NOT NULL,
  `tailor_from_score` tinyint(3) unsigned NOT NULL DEFAULT 60,
  `scan_every_hours` smallint(5) unsigned NOT NULL DEFAULT 24,
  `active` tinyint(1) NOT NULL DEFAULT 1,
  `paused_reason` varchar(500) NOT NULL DEFAULT '',
  `last_scanned_at` int(11) DEFAULT NULL,
  `api_token_hash` char(64) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `api_token_hash` (`api_token_hash`),
  KEY `user_created` (`trongate_user_id`, `created_at`),
  CONSTRAINT `job_searches_user_fk` FOREIGN KEY (`trongate_user_id`) REFERENCES `trongate_users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `job_search_sites` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `job_search_id` int(11) NOT NULL,
  `domain` varchar(255) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `search_domain` (`job_search_id`, `domain`),
  CONSTRAINT `job_search_sites_search_fk` FOREIGN KEY (`job_search_id`) REFERENCES `job_searches` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `job_search_runs` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `job_search_id` int(11) NOT NULL,
  `started_at` int(11) NOT NULL,
  `finished_at` int(11) DEFAULT NULL,
  `posts_found` smallint(5) unsigned NOT NULL DEFAULT 0,
  `posts_new` smallint(5) unsigned NOT NULL DEFAULT 0,
  `input_tokens` int(10) unsigned NOT NULL DEFAULT 0,
  `output_tokens` int(10) unsigned NOT NULL DEFAULT 0,
  `error` varchar(500) NOT NULL DEFAULT '',
  PRIMARY KEY (`id`),
  KEY `search_started` (`job_search_id`, `started_at`),
  CONSTRAINT `job_search_runs_search_fk` FOREIGN KEY (`job_search_id`) REFERENCES `job_searches` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `scanned_job_posts` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `job_search_id` int(11) NOT NULL,
  `job_search_run_id` int(11) DEFAULT NULL,
  `url` varchar(2048) NOT NULL,
  `url_hash` char(40) NOT NULL,
  `title` varchar(255) NOT NULL DEFAULT '',
  `company` varchar(255) NOT NULL DEFAULT '',
  `location` varchar(255) NOT NULL DEFAULT '',
  `status` varchar(12) NOT NULL DEFAULT 'new',
  `error` varchar(500) NOT NULL DEFAULT '',
  `cv_match_id` int(11) DEFAULT NULL,
  `found_at` int(11) NOT NULL,
  `matched_at` int(11) DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `search_url` (`job_search_id`, `url_hash`),
  KEY `search_found` (`job_search_id`, `found_at`),
  CONSTRAINT `scanned_job_posts_search_fk` FOREIGN KEY (`job_search_id`) REFERENCES `job_searches` (`id`) ON DELETE CASCADE,
  CONSTRAINT `scanned_job_posts_run_fk` FOREIGN KEY (`job_search_run_id`) REFERENCES `job_search_runs` (`id`) ON DELETE SET NULL,
  CONSTRAINT `scanned_job_posts_match_fk` FOREIGN KEY (`cv_match_id`) REFERENCES `cv_matches` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `user_llm_keys` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `trongate_user_id` int(11) NOT NULL,
  `provider` varchar(16) NOT NULL,
  `model` varchar(64) NOT NULL DEFAULT '',
  `api_key_encrypted` varchar(1024) NOT NULL,
  `key_hint` varchar(4) NOT NULL,
  `uses` set('resume','application','job_evaluation') NOT NULL DEFAULT 'resume,application,job_evaluation',
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `trongate_user_id` (`trongate_user_id`),
  CONSTRAINT `user_llm_keys_user_fk` FOREIGN KEY (`trongate_user_id`) REFERENCES `trongate_users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

ALTER TABLE `user_llm_keys`
  ADD COLUMN IF NOT EXISTS `uses` set('resume','application','job_evaluation') NOT NULL DEFAULT 'resume,application,job_evaluation' AFTER `key_hint`;

CREATE TABLE IF NOT EXISTS `company_llm_keys` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `company_id` int(11) NOT NULL,
  `provider` varchar(16) NOT NULL,
  `model` varchar(64) NOT NULL DEFAULT '',
  `api_key_encrypted` varchar(1024) NOT NULL,
  `key_hint` varchar(4) NOT NULL,
  `created_at` int(11) NOT NULL,
  `updated_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `company_id` (`company_id`),
  CONSTRAINT `company_llm_keys_company_fk` FOREIGN KEY (`company_id`) REFERENCES `companies` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `queue_jobs` (
  `id` bigint(20) unsigned NOT NULL AUTO_INCREMENT,
  `queue` varchar(32) NOT NULL,
  `module` varchar(64) NOT NULL,
  `method` varchar(100) NOT NULL,
  `parameters` json NOT NULL,
  `unique_key` varchar(191) DEFAULT NULL,
  `available_at` int(11) NOT NULL,
  `attempts` tinyint(3) unsigned NOT NULL DEFAULT 0,
  `reserved_at` int(11) DEFAULT NULL,
  `reserved_by` char(16) DEFAULT NULL,
  `failed_at` int(11) DEFAULT NULL,
  `error_class` varchar(150) DEFAULT NULL,
  `error_message` varchar(1000) DEFAULT NULL,
  `created_at` int(11) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `unique_key` (`unique_key`),
  KEY `due` (`queue`, `failed_at`, `reserved_at`, `available_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;

CREATE TABLE IF NOT EXISTS `queue_workers` (
  `id` char(16) NOT NULL,
  `hostname` varchar(255) NOT NULL,
  `process_id` int(11) NOT NULL,
  `queues` varchar(255) NOT NULL,
  `handled` int(10) unsigned NOT NULL DEFAULT 0,
  `failed` int(10) unsigned NOT NULL DEFAULT 0,
  `started_at` int(11) NOT NULL,
  `last_seen_at` int(11) NOT NULL,
  `stopped_at` int(11) DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `last_seen` (`last_seen_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;
