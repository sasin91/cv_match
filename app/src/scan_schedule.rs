//! When the job scan runs by itself: `recruiter scan --database <this database>` every few hours
//! while the user is signed in, from the system's own scheduler. The schedule is only kept there,
//! so nothing about it is in the database; the setup wizard reads it back. Tests keep it in memory.
//!
//! - Windows: a Task Scheduler task, TASK_NAME, that runs recruiter.exe through
//!   `conhost.exe --headless`, so no console window opens on the desktop each time it runs.
//! - Linux: a systemd user timer, SYSTEMD_UNIT.timer, starting SYSTEMD_UNIT.service, in
//!   ~/.config/systemd/user. The timer file notes its interval, which is read back from it.
//! - Elsewhere (macOS for now) the schedule says it isn't there yet.

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
#[cfg_attr(not(windows), allow(dead_code))]
const TASK_RUN_MAX_LENGTH: usize = 261;

/// The systemd user units' name: recruiter-scan.timer starts recruiter-scan.service.
pub const SYSTEMD_UNIT: &str = "recruiter-scan";

/// What the timer file's interval line starts with; the rest is the hours.
const SYSTEMD_INTERVAL_NOTE: &str = "# Recruiter's scan interval in hours: ";

/// The scan command the task runs.
#[derive(Debug, Clone)]
pub struct ScanCommand {
    pub exe: PathBuf,
    pub database: PathBuf,
}

impl ScanCommand {
    /// What the scheduler runs, as the wizard shows it.
    pub fn task_run(&self) -> String {
        if cfg!(windows) {
            self.windows_task_run()
        } else {
            self.exec_start()
        }
    }

    /// The task's command line (schtasks /TR): recruiter.exe scan, with this database, through a
    /// console without a window.
    pub fn windows_task_run(&self) -> String {
        format!(
            r#"conhost.exe --headless "{}" scan --database "{}""#,
            self.exe.display(),
            self.database.display()
        )
    }

    /// The systemd service's ExecStart: the program, scan, and this database.
    pub fn exec_start(&self) -> String {
        format!(
            r#""{}" scan --database "{}""#,
            self.exe.display(),
            self.database.display()
        )
    }

    /// The systemd user service that runs the scan once.
    pub fn systemd_service(&self) -> String {
        format!(
            "[Unit]\nDescription=Recruiter's job scan\n\n[Service]\nType=oneshot\nExecStart={}\n",
            self.exec_start()
        )
    }
}

/// The systemd user timer that starts the scan every hours (daily at DAILY_START for a day).
pub fn systemd_timer(hours: u32) -> String {
    let calendar = if hours >= HOURS_IN_DAY {
        format!("*-*-* {DAILY_START}:00")
    } else {
        // Every hours from midnight: 0/4 is 00, 04, 08 ... (the offered intervals divide a day).
        format!("*-*-* 0/{hours}:00:00")
    };
    format!(
        "{SYSTEMD_INTERVAL_NOTE}{hours}\n[Unit]\nDescription=Recruiter's job scan, {}\n\n\
         [Timer]\nOnCalendar={calendar}\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n",
        INTERVALS
            .iter()
            .find(|(offered, _)| *offered == hours)
            .map(|(_, label)| label.to_lowercase())
            .unwrap_or_default()
    )
}

/// The interval a timer file notes, in hours; None for a file without the note.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn systemd_interval_in(timer: &str) -> Option<u32> {
    timer
        .lines()
        .find_map(|line| line.strip_prefix(SYSTEMD_INTERVAL_NOTE))
        .and_then(|hours| hours.trim().parse().ok())
}

pub struct ScanSchedule {
    store: Store,
}

enum Store {
    /// The system's scheduler: Task Scheduler, or a systemd user timer.
    System(ScanCommand),
    /// The registered interval in hours, for tests.
    InMemory(Mutex<Option<u32>>),
}

impl ScanSchedule {
    /// The system's scheduler, running exe's scan on database.
    pub fn system(exe: &Path, database: &Path) -> Self {
        ScanSchedule {
            store: Store::System(ScanCommand {
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
            Store::System(command) => command.task_run(),
            Store::InMemory(_) => String::new(),
        }
    }

    /// The registered task's interval in hours, or None when there is no task.
    pub async fn interval_hours(&self) -> Result<Option<u32>, String> {
        match &self.store {
            Store::System(_) => system::interval_hours().await,
            Store::InMemory(interval) => Ok(*lock(interval)),
        }
    }

    /// Registers the task to run every hours, replacing the one registered before.
    pub async fn register(&self, hours: u32) -> Result<(), String> {
        if !INTERVALS.iter().any(|(offered, _)| *offered == hours) {
            return Err(format!("{hours} hours isn't one of the intervals offered."));
        }
        match &self.store {
            Store::System(command) => system::register(command, hours).await,
            Store::InMemory(interval) => {
                *lock(interval) = Some(hours);
                Ok(())
            }
        }
    }

    /// Removes the task, so the scan only runs from the Scan now button.
    pub async fn remove(&self) -> Result<(), String> {
        match &self.store {
            Store::System(_) => system::remove().await,
            Store::InMemory(interval) => {
                *lock(interval) = None;
                Ok(())
            }
        }
    }
}

/// schtasks' /SC and /MO (and /ST for daily) for a scan every hours; modifier is hours as text.
#[cfg_attr(not(windows), allow(dead_code))]
fn schedule_args(hours: u32, modifier: &str) -> Vec<&str> {
    if hours >= HOURS_IN_DAY {
        vec!["/SC", "DAILY", "/MO", "1", "/ST", DAILY_START]
    } else {
        vec!["/SC", "HOURLY", "/MO", modifier]
    }
}

/// The interval of a task as `schtasks /Query /XML` prints it: an hourly task repeats every
/// "PT4H", a daily one is scheduled by day. None when it is neither.
#[cfg_attr(not(windows), allow(dead_code))]
fn interval_in(xml: &str) -> Option<u32> {
    if xml.contains("<ScheduleByDay>") {
        return Some(HOURS_IN_DAY);
    }
    let start = xml.find("<Interval>PT")? + "<Interval>PT".len();
    let hours = xml[start..].split('H').next()?;
    hours.parse().ok()
}

/// Windows: Task Scheduler, through schtasks.exe.
#[cfg(windows)]
mod system {
    use super::*;

    pub async fn interval_hours() -> Result<Option<u32>, String> {
        let answer = schtasks(&["/Query", "/TN", TASK_NAME, "/XML"]).await?;
        Ok(answer
            .ok()
            .map(|xml| interval_in(&xml).unwrap_or(DEFAULT_INTERVAL_HOURS)))
    }

    pub async fn register(command: &ScanCommand, hours: u32) -> Result<(), String> {
        let task_run = command.windows_task_run();
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

    pub async fn remove() -> Result<(), String> {
        schtasks(&["/Delete", "/F", "/TN", TASK_NAME]).await?.map(|_| ())
    }

    /// Runs schtasks.exe: Ok(Ok(stdout)) when it succeeded, Ok(Err(stderr)) when it answered
    /// with a failure (like "the task doesn't exist"), Err when it couldn't run at all.
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
}

/// Linux: a systemd user timer, its files in ~/.config/systemd/user.
#[cfg(target_os = "linux")]
mod system {
    use super::*;

    /// ~/.config/systemd/user
    fn units_folder() -> Result<PathBuf, String> {
        dirs::config_dir()
            .map(|config| config.join("systemd").join("user"))
            .ok_or_else(|| "Your home folder couldn't be found, so there's nowhere to keep the timer.".to_string())
    }

    fn unit(kind: &str) -> Result<PathBuf, String> {
        Ok(units_folder()?.join(format!("{SYSTEMD_UNIT}.{kind}")))
    }

    pub async fn interval_hours() -> Result<Option<u32>, String> {
        match tokio::fs::read_to_string(unit("timer")?).await {
            Ok(timer) => Ok(Some(systemd_interval_in(&timer).unwrap_or(DEFAULT_INTERVAL_HOURS))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("The job scan's timer couldn't be read: {e}")),
        }
    }

    pub async fn register(command: &ScanCommand, hours: u32) -> Result<(), String> {
        let folder = units_folder()?;
        tokio::fs::create_dir_all(&folder)
            .await
            .map_err(|e| format!("{} couldn't be made: {e}", folder.display()))?;
        for (kind, text) in [("service", command.systemd_service()), ("timer", systemd_timer(hours))] {
            let path = unit(kind)?;
            tokio::fs::write(&path, text)
                .await
                .map_err(|e| format!("{} couldn't be written: {e}", path.display()))?;
        }
        let started = match systemctl(&["daemon-reload"]).await {
            Ok(()) => systemctl(&["enable", "--now", &format!("{SYSTEMD_UNIT}.timer")]).await,
            Err(why) => Err(why),
        };
        if started.is_err() {
            // Files of a timer that doesn't run would read back as a schedule.
            for kind in ["timer", "service"] {
                let _ = tokio::fs::remove_file(unit(kind)?).await;
            }
        }
        started
    }

    pub async fn remove() -> Result<(), String> {
        // A timer that isn't loaded (no systemd here, say) is still removed by its files.
        if let Err(why) = systemctl(&["disable", "--now", &format!("{SYSTEMD_UNIT}.timer")]).await {
            eprintln!("scan_schedule: {why}");
        }
        for kind in ["timer", "service"] {
            match tokio::fs::remove_file(unit(kind)?).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("The job scan's {kind} couldn't be removed: {e}")),
            }
        }
        if let Err(why) = systemctl(&["daemon-reload"]).await {
            eprintln!("scan_schedule: {why}");
        }
        Ok(())
    }

    /// Runs `systemctl --user args`; why not, when it fails.
    async fn systemctl(args: &[&str]) -> Result<(), String> {
        let output = tokio::process::Command::new("systemctl")
            .arg("--user")
            .args(args)
            .output()
            .await
            .map_err(|e| {
                format!("systemctl couldn't run ({e}). The job scan's schedule is a systemd user timer, so it needs systemd.")
            })?;
        if output.status.success() {
            return Ok(());
        }
        let why = String::from_utf8_lossy(&output.stderr).trim().to_string();
        eprintln!("scan_schedule: systemctl --user {}: {why}", args.join(" "));
        Err(format!("systemd said: {why}"))
    }
}

/// Elsewhere: no scheduler yet.
#[cfg(not(any(windows, target_os = "linux")))]
mod system {
    use super::*;

    const NOT_HERE: &str =
        "The job scan can't be scheduled on this system yet. Run `recruiter scan` from cron or launchd instead.";

    pub async fn interval_hours() -> Result<Option<u32>, String> {
        Ok(None)
    }

    pub async fn register(_: &ScanCommand, _: u32) -> Result<(), String> {
        Err(NOT_HERE.to_string())
    }

    pub async fn remove() -> Result<(), String> {
        Err(NOT_HERE.to_string())
    }
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
            command.windows_task_run(),
            r#"conhost.exe --headless "C:\Apps\recruiter.exe" scan --database "C:\Users\me\AppData\Roaming\Recruiter\recruiter.sqlite""#
        );
    }

    #[test]
    fn the_systemd_timer_notes_its_interval_and_runs_on_the_hour() {
        let every_four = systemd_timer(4);
        assert!(every_four.contains("OnCalendar=*-*-* 0/4:00:00"), "{every_four}");
        assert!(
            every_four.contains("Description=Recruiter's job scan, every 4 hours"),
            "{every_four}"
        );
        assert_eq!(systemd_interval_in(&every_four), Some(4));
        let daily = systemd_timer(HOURS_IN_DAY);
        assert!(daily.contains("OnCalendar=*-*-* 09:00:00"), "{daily}");
        assert_eq!(systemd_interval_in(&daily), Some(HOURS_IN_DAY));
        assert_eq!(systemd_interval_in("[Timer]\nOnCalendar=daily\n"), None);
    }

    #[test]
    fn the_systemd_service_runs_the_scan_on_this_database() {
        let command = ScanCommand {
            exe: PathBuf::from("/home/me/.local/lib/recruiter/recruiter"),
            database: PathBuf::from("/home/me/My data/recruiter.sqlite"),
        };
        assert!(
            command.systemd_service().contains(
                r#"ExecStart="/home/me/.local/lib/recruiter/recruiter" scan --database "/home/me/My data/recruiter.sqlite""#
            ),
            "{}",
            command.systemd_service()
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
