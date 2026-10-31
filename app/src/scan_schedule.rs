//! When the job scan runs by itself: a Windows Task Scheduler task, TASK_NAME, that runs
//! `recruiter scan --database <this database>` every few hours while the user is signed in to
//! Windows. The task is the only record of the schedule, so nothing about it is in the database;
//! the setup wizard reads it back from Task Scheduler. Tests keep it in memory instead.
//!
//! The task runs recruiter.exe through `conhost.exe --headless`, so no console window opens on
//! the desktop each time it runs.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The task's name in Task Scheduler, in a Recruiter folder.
pub const TASK_NAME: &str = r"Recruiter\Job scan";

/// How often the scan can run, in hours, with what the wizard calls it.
pub const INTERVALS: [(u32, &str); 4] = [
    (2, "Every 2 hours"),
    (4, "Every 4 hours"),
    (8, "Every 8 hours"),
    (HOURS_IN_DAY, "Once a day"),
];

/// The interval the wizard picks unless the task has another, in hours.
pub const DEFAULT_INTERVAL_HOURS: u32 = 4;

/// Hours in a day: the interval that is scheduled daily rather than hourly.
const HOURS_IN_DAY: u32 = 24;

/// When a daily scan runs, as schtasks /ST takes it.
const DAILY_START: &str = "09:00";

/// Longest command schtasks /TR takes, in characters.
const TASK_RUN_MAX_LENGTH: usize = 261;

/// The scan command the task runs.
#[derive(Debug, Clone)]
pub struct ScanCommand {
    pub exe: PathBuf,
    pub database: PathBuf,
}

impl ScanCommand {
    /// The task's command line (schtasks /TR): recruiter.exe scan, with this database, through a
    /// console without a window.
    pub fn task_run(&self) -> String {
        format!(
            r#"conhost.exe --headless "{}" scan --database "{}""#,
            self.exe.display(),
            self.database.display()
        )
    }
}

pub struct ScanSchedule {
    store: Store,
}

enum Store {
    TaskScheduler(ScanCommand),
    /// The registered interval in hours, for tests.
    InMemory(Mutex<Option<u32>>),
}

impl ScanSchedule {
    /// Task Scheduler's TASK_NAME, running this command.
    pub fn task_scheduler(exe: &Path, database: &Path) -> Self {
        ScanSchedule {
            store: Store::TaskScheduler(ScanCommand {
                exe: exe.to_path_buf(),
                database: database.to_path_buf(),
            }),
        }
    }

    pub fn in_memory() -> Self {
        ScanSchedule {
            store: Store::InMemory(Mutex::new(None)),
        }
    }

    /// What the task runs, for the wizard to show; "" in memory.
    pub fn task_run(&self) -> String {
        match &self.store {
            Store::TaskScheduler(command) => command.task_run(),
            Store::InMemory(_) => String::new(),
        }
    }

    /// The registered task's interval in hours, or None when there is no task.
    pub async fn interval_hours(&self) -> Result<Option<u32>, String> {
        match &self.store {
            Store::TaskScheduler(_) => {
                let answer = schtasks(&["/Query", "/TN", TASK_NAME, "/XML"]).await?;
                Ok(answer
                    .ok()
                    .map(|xml| interval_in(&xml).unwrap_or(DEFAULT_INTERVAL_HOURS)))
            }
            Store::InMemory(interval) => Ok(*lock(interval)),
        }
    }

    /// Registers the task to run every hours, replacing the one registered before.
    pub async fn register(&self, hours: u32) -> Result<(), String> {
        if !INTERVALS.iter().any(|(offered, _)| *offered == hours) {
            return Err(format!("{hours} hours isn't one of the intervals offered."));
        }
        match &self.store {
            Store::TaskScheduler(command) => {
                let task_run = command.task_run();
                if task_run.chars().count() > TASK_RUN_MAX_LENGTH {
                    return Err(format!(
                        "Task Scheduler takes a command of at most {TASK_RUN_MAX_LENGTH} characters, and {task_run} is longer. Move the app or its database to a shorter path."
                    ));
                }
                let mut args = vec!["/Create", "/F", "/TN", TASK_NAME, "/TR", task_run.as_str()];
                let modifier = hours.to_string();
                args.extend(schedule_args(hours, &modifier));
                schtasks(&args).await?.map(|_| ())
            }
            Store::InMemory(interval) => {
                *lock(interval) = Some(hours);
                Ok(())
            }
        }
    }

    /// Removes the task, so the scan only runs from the Scan now button.
    pub async fn remove(&self) -> Result<(), String> {
        match &self.store {
            Store::TaskScheduler(_) => schtasks(&["/Delete", "/F", "/TN", TASK_NAME]).await?.map(|_| ()),
            Store::InMemory(interval) => {
                *lock(interval) = None;
                Ok(())
            }
        }
    }
}

/// schtasks' /SC and /MO (and /ST for daily) for a scan every hours; modifier is hours as text.
fn schedule_args(hours: u32, modifier: &str) -> Vec<&str> {
    if hours >= HOURS_IN_DAY {
        vec!["/SC", "DAILY", "/MO", "1", "/ST", DAILY_START]
    } else {
        vec!["/SC", "HOURLY", "/MO", modifier]
    }
}

/// The interval of a task as `schtasks /Query /XML` prints it: an hourly task repeats every
/// "PT4H", a daily one is scheduled by day. None when it is neither.
fn interval_in(xml: &str) -> Option<u32> {
    if xml.contains("<ScheduleByDay>") {
        return Some(HOURS_IN_DAY);
    }
    let start = xml.find("<Interval>PT")? + "<Interval>PT".len();
    let hours = xml[start..].split('H').next()?;
    hours.parse().ok()
}

/// Runs schtasks.exe: Ok(Ok(stdout)) when it succeeded, Ok(Err(stderr)) when it answered with a
/// failure (like "the task doesn't exist"), Err when it couldn't run at all.
#[cfg(windows)]
async fn schtasks(args: &[&str]) -> Result<Result<String, String>, String> {
    let output = tokio::process::Command::new("schtasks.exe")
        .args(args)
        .output()
        .await
        .map_err(|e| {
            eprintln!("scan_schedule: schtasks.exe couldn't run: {e}");
            format!("Task Scheduler (schtasks.exe) couldn't run: {e}")
        })?;
    if output.status.success() {
        return Ok(Ok(String::from_utf8_lossy(&output.stdout).into_owned()));
    }
    let why = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if args.first() != Some(&"/Query") {
        eprintln!("scan_schedule: schtasks {}: {why}", args.join(" "));
    }
    Ok(Err(format!("Task Scheduler said: {why}")))
}

#[cfg(not(windows))]
async fn schtasks(args: &[&str]) -> Result<Result<String, String>, String> {
    if args.first() == Some(&"/Query") {
        return Ok(Err(String::new()));
    }
    Err("The job scan's schedule is a Windows Task Scheduler task, and this isn't Windows. Schedule `recruiter scan` with cron instead.".to_string())
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interval_is_read_back_from_the_tasks_xml() {
        let hourly =
            "<Triggers><TimeTrigger><Repetition><Interval>PT4H</Interval></Repetition></TimeTrigger></Triggers>";
        assert_eq!(interval_in(hourly), Some(4));
        let daily = "<CalendarTrigger><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger>";
        assert_eq!(interval_in(daily), Some(HOURS_IN_DAY));
        assert_eq!(interval_in("<Task/>"), None);
    }

    #[test]
    fn a_daily_scan_is_scheduled_by_day_and_the_others_by_the_hour() {
        assert_eq!(schedule_args(4, "4"), ["/SC", "HOURLY", "/MO", "4"]);
        assert_eq!(
            schedule_args(HOURS_IN_DAY, "24"),
            ["/SC", "DAILY", "/MO", "1", "/ST", DAILY_START]
        );
    }

    #[test]
    fn the_task_runs_the_scan_on_this_database_without_a_window() {
        let command = ScanCommand {
            exe: PathBuf::from(r"C:\Apps\recruiter.exe"),
            database: PathBuf::from(r"C:\Users\me\AppData\Roaming\Recruiter\recruiter.sqlite"),
        };
        assert_eq!(
            command.task_run(),
            r#"conhost.exe --headless "C:\Apps\recruiter.exe" scan --database "C:\Users\me\AppData\Roaming\Recruiter\recruiter.sqlite""#
        );
    }

    #[tokio::test]
    async fn only_the_offered_intervals_are_registered() {
        let schedule = ScanSchedule::in_memory();
        assert_eq!(schedule.interval_hours().await.unwrap(), None);
        assert!(schedule.register(3).await.is_err());
        schedule.register(8).await.unwrap();
        assert_eq!(schedule.interval_hours().await.unwrap(), Some(8));
        schedule.remove().await.unwrap();
        assert_eq!(schedule.interval_hours().await.unwrap(), None);
    }
}
