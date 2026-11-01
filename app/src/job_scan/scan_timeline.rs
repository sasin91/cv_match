//! A scan's timeline as a search's page shows it (run_steps): the scan's own steps (the sites
//! read, the web search, the posts listed), then each post's (its page read, read by the model,
//! matched, written) under the post. A step the model took part in can be opened to show what it
//! was asked and answered: the chat outline.

use super::run_steps::Step;

/// Milliseconds in a second.
const MS_PER_SECOND: i64 = 1000;
/// Seconds in a minute.
const SECONDS_PER_MINUTE: i64 = 60;

/// The timeline of one scan.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanTimeline {
    /// The scan's own steps.
    pub steps: Vec<TimelineStep>,
    /// Each post's, in the order they were matched.
    pub posts: Vec<TimelinePost>,
}

/// A post on the timeline.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimelinePost {
    /// "Post 2: PHP-udvikler", or its link without a title.
    pub heading: String,
    pub steps: Vec<TimelineStep>,
}

/// One step as the timeline prints it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimelineStep {
    pub id: i64,
    /// "16:02:11".
    pub at: String,
    /// "1.2 s", "2 min 5 s".
    pub took: String,
    pub text: String,
    pub failed: bool,
    /// The model took part: it can be opened.
    pub asked_model: bool,
}

impl ScanTimeline {
    pub fn from(steps: &[Step]) -> Self {
        let mut timeline = ScanTimeline::default();
        let mut current: Option<i64> = None;
        for step in steps {
            let shown = TimelineStep::from(step);
            match step.post_id {
                None => timeline.steps.push(shown),
                Some(post) if current == Some(post) => {
                    if let Some(last) = timeline.posts.last_mut() {
                        last.steps.push(shown);
                    }
                }
                Some(post) => {
                    current = Some(post);
                    let number = timeline.posts.len() + 1;
                    timeline.posts.push(TimelinePost {
                        heading: post_heading(number, step),
                        steps: vec![shown],
                    });
                }
            }
        }
        timeline
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.posts.is_empty()
    }
}

impl TimelineStep {
    fn from(step: &Step) -> Self {
        TimelineStep {
            id: step.id,
            at: clock(step.started_at_ms),
            took: took(step.took_ms),
            text: step.text.clone(),
            failed: step.failed,
            asked_model: step.asked_model,
        }
    }
}

/// "Post 2: PHP-udvikler", or its link while it has no title.
fn post_heading(number: usize, step: &Step) -> String {
    let named = if step.post_title.trim().is_empty() {
        step.post_url.as_str()
    } else {
        step.post_title.trim()
    };
    format!("Post {number}: {named}")
}

/// "16:02:11" in this computer's time zone, as the page's other times.
fn clock(at_ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(at_ms)
        .map(|at| at.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

/// "0.4 s", "12 s", "2 min 5 s".
fn took(ms: i64) -> String {
    let seconds = ms / MS_PER_SECOND;
    if seconds < 10 {
        return format!("{:.1} s", ms as f64 / MS_PER_SECOND as f64);
    }
    if seconds < SECONDS_PER_MINUTE {
        return format!("{seconds} s");
    }
    format!(
        "{} min {} s",
        seconds / SECONDS_PER_MINUTE,
        seconds % SECONDS_PER_MINUTE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: i64, post: Option<i64>, text: &str) -> Step {
        Step {
            id,
            post_id: post,
            text: text.to_string(),
            took_ms: 1500,
            post_title: if post == Some(7) {
                "PHP-udvikler".to_string()
            } else {
                String::new()
            },
            post_url: "https://jobnet.dk/find-job/1".to_string(),
            ..Step::default()
        }
    }

    #[test]
    fn a_posts_steps_go_under_the_post() {
        let timeline = ScanTimeline::from(&[
            step(1, None, "jobnet.dk: 20 posts."),
            step(2, None, "20 posts listed, 2 new."),
            step(3, Some(7), "Read"),
            step(4, Some(7), "Matched"),
            step(5, Some(8), "Read"),
        ]);
        assert_eq!(timeline.steps.len(), 2);
        assert_eq!(timeline.posts.len(), 2);
        assert_eq!(timeline.posts[0].heading, "Post 1: PHP-udvikler");
        assert_eq!(timeline.posts[0].steps.len(), 2);
        assert_eq!(timeline.posts[1].heading, "Post 2: https://jobnet.dk/find-job/1");
        assert_eq!(timeline.steps[0].took, "1.5 s");
        assert!(ScanTimeline::from(&[]).is_empty());
    }

    #[test]
    fn a_steps_time_reads_as_seconds_or_minutes() {
        assert_eq!(took(400), "0.4 s");
        assert_eq!(took(12_300), "12 s");
        assert_eq!(took(125_000), "2 min 5 s");
        let shown = clock(1_793_545_331_000);
        assert_eq!(shown.len(), "16:02:11".len());
        assert!(shown.ends_with(":02:11"), "{shown}");
    }
}
