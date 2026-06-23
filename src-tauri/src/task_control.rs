use crate::domain::ActiveShardProgress;
use rusqlite::Connection;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::Notify;

#[derive(Debug, Clone)]
pub(crate) struct ActiveTask {
    pub(crate) job_type: String,
    profile_ids: HashSet<String>,
}

#[derive(Default)]
pub(crate) struct ActiveTaskRegistry {
    tasks: Mutex<HashMap<String, ActiveTask>>,
}

pub(crate) struct ActiveTaskPermit<'a> {
    registry: &'a ActiveTaskRegistry,
    novel_id: String,
}

impl ActiveTaskRegistry {
    pub(crate) fn acquire<'a, I, S>(
        &'a self,
        novel_id: &str,
        profile_ids: I,
        job_type: &str,
    ) -> Result<ActiveTaskPermit<'a>, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut tasks = self.tasks.lock().map_err(|error| error.to_string())?;
        if let Some(task) = tasks.get(novel_id) {
            return Err(format!(
                "《当前小说》已有{}任务正在运行，请等待任务结束或先终止任务。",
                task.job_type
            ));
        }
        tasks.insert(
            novel_id.to_string(),
            ActiveTask {
                job_type: job_type.to_string(),
                profile_ids: profile_ids
                    .into_iter()
                    .map(|profile_id| profile_id.as_ref().to_string())
                    .collect(),
            },
        );
        Ok(ActiveTaskPermit {
            registry: self,
            novel_id: novel_id.to_string(),
        })
    }

    pub(crate) fn novel_is_active(&self, novel_id: &str) -> Result<bool, String> {
        let tasks = self.tasks.lock().map_err(|error| error.to_string())?;
        Ok(tasks.contains_key(novel_id))
    }

    pub(crate) fn profile_is_active(&self, profile_id: &str) -> Result<bool, String> {
        let tasks = self.tasks.lock().map_err(|error| error.to_string())?;
        Ok(tasks
            .values()
            .any(|task| task.profile_ids.contains(profile_id)))
    }

    pub(crate) fn any_active(&self) -> Result<bool, String> {
        let tasks = self.tasks.lock().map_err(|error| error.to_string())?;
        Ok(!tasks.is_empty())
    }
}

impl Drop for ActiveTaskPermit<'_> {
    fn drop(&mut self) {
        if let Ok(mut tasks) = self.registry.tasks.lock() {
            tasks.remove(&self.novel_id);
        }
    }
}

#[derive(Default)]
pub(crate) struct CancellableTaskRegistry {
    tasks: Mutex<HashMap<String, Arc<CancellationSignal>>>,
}

#[derive(Default)]
struct CancellationSignal {
    cancelled: AtomicBool,
    notify: Notify,
}

pub(crate) struct CancellableTaskPermit<'a> {
    registry: &'a CancellableTaskRegistry,
    novel_id: String,
    signal: Arc<CancellationSignal>,
}

impl CancellableTaskRegistry {
    pub(crate) fn register(&self, novel_id: &str) -> Result<CancellableTaskPermit<'_>, String> {
        let mut tasks = self.tasks.lock().map_err(|error| error.to_string())?;
        if tasks.contains_key(novel_id) {
            return Err("当前小说已有可终止的单章重写任务。".to_string());
        }
        let signal = Arc::new(CancellationSignal::default());
        tasks.insert(novel_id.to_string(), signal.clone());
        Ok(CancellableTaskPermit {
            registry: self,
            novel_id: novel_id.to_string(),
            signal,
        })
    }

    pub(crate) fn cancel(&self, novel_id: &str) -> Result<bool, String> {
        let signal = {
            let tasks = self.tasks.lock().map_err(|error| error.to_string())?;
            tasks.get(novel_id).cloned()
        };
        let Some(signal) = signal else {
            return Ok(false);
        };
        signal.cancelled.store(true, Ordering::Release);
        signal.notify.notify_waiters();
        Ok(true)
    }

    pub(crate) fn any_active(&self) -> Result<bool, String> {
        let tasks = self.tasks.lock().map_err(|error| error.to_string())?;
        Ok(!tasks.is_empty())
    }
}

impl CancellableTaskPermit<'_> {
    pub(crate) async fn cancelled(&self) {
        loop {
            if self.signal.cancelled.load(Ordering::Acquire) {
                return;
            }
            let notified = self.signal.notify.notified();
            if self.signal.cancelled.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

impl Drop for CancellableTaskPermit<'_> {
    fn drop(&mut self) {
        if let Ok(mut tasks) = self.registry.tasks.lock() {
            if tasks
                .get(&self.novel_id)
                .is_some_and(|signal| Arc::ptr_eq(signal, &self.signal))
            {
                tasks.remove(&self.novel_id);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AutoRunControl {
    pub(crate) status: String,
    pub(crate) start_batch_index: i64,
    pub(crate) completed_batches: i64,
    pub(crate) job_id: Option<String>,
    pub(crate) profile_ids: HashSet<String>,
    pub(crate) recoverable: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AutoRunProgressState {
    pub(crate) phase: Option<String>,
    pub(crate) batch_index: Option<i64>,
    pub(crate) batch_total: Option<i64>,
    pub(crate) batch_label: Option<String>,
    pub(crate) shard_total: usize,
    pub(crate) completed_shards: HashSet<usize>,
    pub(crate) chapter_total: usize,
    pub(crate) completed_chapter_ids: HashSet<String>,
    pub(crate) active_shards: BTreeMap<usize, ActiveShardProgress>,
}

pub(crate) struct AutoRunCleanup<'a> {
    runs: &'a Mutex<HashMap<String, AutoRunControl>>,
    progress: &'a Mutex<HashMap<String, AutoRunProgressState>>,
    conn: &'a Mutex<Connection>,
    novel_id: String,
}

impl<'a> AutoRunCleanup<'a> {
    pub(crate) fn new(
        runs: &'a Mutex<HashMap<String, AutoRunControl>>,
        progress: &'a Mutex<HashMap<String, AutoRunProgressState>>,
        conn: &'a Mutex<Connection>,
        novel_id: &str,
    ) -> Self {
        Self {
            runs,
            progress,
            conn,
            novel_id: novel_id.to_string(),
        }
    }
}

impl Drop for AutoRunCleanup<'_> {
    fn drop(&mut self) {
        let should_cleanup = if let Ok(mut runs) = self.runs.lock() {
            let should_cleanup = runs
                .get(&self.novel_id)
                .is_none_or(|control| control.status != "paused");
            if should_cleanup {
                runs.remove(&self.novel_id);
            }
            should_cleanup
        } else {
            false
        };
        if !should_cleanup {
            return;
        }
        if let Ok(mut progress) = self.progress.lock() {
            progress.remove(&self.novel_id);
        }
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "DELETE FROM auto_run_checkpoints WHERE novel_id = ?1",
                [&self.novel_id],
            );
        }
    }
}

pub(crate) fn should_terminate_paused_run(current_status: &str, requested_status: &str) -> bool {
    current_status == "paused" && requested_status == "terminate_requested"
}

pub(crate) fn auto_runs_have_non_paused(
    runs: &Mutex<HashMap<String, AutoRunControl>>,
) -> Result<bool, String> {
    let runs = runs.lock().map_err(|error| error.to_string())?;
    Ok(runs.values().any(|control| control.status != "paused"))
}

pub(crate) fn auto_runs_are_only_paused(
    runs: &Mutex<HashMap<String, AutoRunControl>>,
) -> Result<bool, String> {
    let runs = runs.lock().map_err(|error| error.to_string())?;
    Ok(!runs.is_empty() && runs.values().all(|control| control.status == "paused"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_task_permit_blocks_duplicates_and_releases_on_drop() {
        let registry = ActiveTaskRegistry::default();
        let permit = registry
            .acquire("novel-1", ["profile-1"], "分析")
            .expect("first task");
        assert!(registry.acquire("novel-1", ["profile-2"], "改写").is_err());
        assert!(registry.novel_is_active("novel-1").expect("active novel"));
        assert!(registry
            .profile_is_active("profile-1")
            .expect("active profile"));
        drop(permit);
        assert!(!registry.novel_is_active("novel-1").expect("released novel"));
    }

    #[tokio::test]
    async fn cancellable_task_notifies_and_releases_registration() {
        let registry = CancellableTaskRegistry::default();
        let permit = registry.register("novel-1").expect("register task");
        assert!(registry.register("novel-1").is_err());
        assert!(registry.cancel("novel-1").expect("cancel task"));
        permit.cancelled().await;
        drop(permit);
        assert!(!registry.cancel("novel-1").expect("task released"));
        assert!(registry.register("novel-1").is_ok());
    }

    #[test]
    fn auto_run_cleanup_preserves_only_paused_state() {
        let runs = Mutex::new(HashMap::from([(
            "novel-1".to_string(),
            AutoRunControl {
                status: "running".to_string(),
                start_batch_index: 0,
                completed_batches: 0,
                job_id: None,
                profile_ids: HashSet::new(),
                recoverable: true,
            },
        )]));
        let progress = Mutex::new(HashMap::from([(
            "novel-1".to_string(),
            AutoRunProgressState::default(),
        )]));
        let conn = Mutex::new(Connection::open_in_memory().expect("open database"));
        conn.lock()
            .expect("database")
            .execute_batch(
                "CREATE TABLE auto_run_checkpoints (novel_id TEXT PRIMARY KEY);
                 INSERT INTO auto_run_checkpoints VALUES ('novel-1');",
            )
            .expect("seed checkpoint");
        drop(AutoRunCleanup::new(&runs, &progress, &conn, "novel-1"));
        assert!(runs.lock().expect("runs").is_empty());
        assert!(progress.lock().expect("progress").is_empty());
        let checkpoint_count: i64 = conn
            .lock()
            .expect("database")
            .query_row("SELECT COUNT(*) FROM auto_run_checkpoints", [], |row| {
                row.get(0)
            })
            .expect("count checkpoints");
        assert_eq!(checkpoint_count, 0);

        runs.lock().expect("runs").insert(
            "novel-1".to_string(),
            AutoRunControl {
                status: "paused".to_string(),
                start_batch_index: 0,
                completed_batches: 1,
                job_id: None,
                profile_ids: HashSet::new(),
                recoverable: true,
            },
        );
        conn.lock()
            .expect("database")
            .execute("INSERT INTO auto_run_checkpoints VALUES ('novel-1')", [])
            .expect("restore checkpoint");
        drop(AutoRunCleanup::new(&runs, &progress, &conn, "novel-1"));
        assert!(runs.lock().expect("runs").contains_key("novel-1"));
        let checkpoint_count: i64 = conn
            .lock()
            .expect("database")
            .query_row("SELECT COUNT(*) FROM auto_run_checkpoints", [], |row| {
                row.get(0)
            })
            .expect("count checkpoints");
        assert_eq!(checkpoint_count, 1);
    }

    #[test]
    fn terminating_a_paused_run_must_finish_immediately() {
        assert!(should_terminate_paused_run("paused", "terminate_requested"));
        assert!(!should_terminate_paused_run(
            "running",
            "terminate_requested"
        ));
        assert!(!should_terminate_paused_run("paused", "pause_requested"));
    }

    #[test]
    fn distinguishes_paused_auto_runs_from_running_auto_runs() {
        let runs = Mutex::new(HashMap::from([(
            "novel-1".to_string(),
            AutoRunControl {
                status: "paused".to_string(),
                start_batch_index: 0,
                completed_batches: 1,
                job_id: None,
                profile_ids: HashSet::new(),
                recoverable: true,
            },
        )]));
        assert!(!auto_runs_have_non_paused(&runs).expect("paused only"));
        assert!(auto_runs_are_only_paused(&runs).expect("paused only"));

        runs.lock().expect("runs").insert(
            "novel-2".to_string(),
            AutoRunControl {
                status: "running".to_string(),
                start_batch_index: 0,
                completed_batches: 0,
                job_id: None,
                profile_ids: HashSet::new(),
                recoverable: true,
            },
        );
        assert!(auto_runs_have_non_paused(&runs).expect("has running"));
        assert!(!auto_runs_are_only_paused(&runs).expect("has running"));
    }
}
