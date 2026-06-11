use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

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

#[derive(Debug, Clone)]
pub(crate) struct AutoRunControl {
    pub(crate) status: String,
    pub(crate) completed_batches: i64,
    pub(crate) job_id: Option<String>,
    pub(crate) profile_ids: HashSet<String>,
}

pub(crate) struct AutoRunCleanup<'a> {
    runs: &'a Mutex<HashMap<String, AutoRunControl>>,
    novel_id: String,
}

impl<'a> AutoRunCleanup<'a> {
    pub(crate) fn new(runs: &'a Mutex<HashMap<String, AutoRunControl>>, novel_id: &str) -> Self {
        Self {
            runs,
            novel_id: novel_id.to_string(),
        }
    }
}

impl Drop for AutoRunCleanup<'_> {
    fn drop(&mut self) {
        if let Ok(mut runs) = self.runs.lock() {
            if runs
                .get(&self.novel_id)
                .is_none_or(|control| control.status != "paused")
            {
                runs.remove(&self.novel_id);
            }
        }
    }
}

pub(crate) fn should_terminate_paused_run(current_status: &str, requested_status: &str) -> bool {
    current_status == "paused" && requested_status == "terminate_requested"
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

    #[test]
    fn auto_run_cleanup_preserves_only_paused_state() {
        let runs = Mutex::new(HashMap::from([(
            "novel-1".to_string(),
            AutoRunControl {
                status: "running".to_string(),
                completed_batches: 0,
                job_id: None,
                profile_ids: HashSet::new(),
            },
        )]));
        drop(AutoRunCleanup::new(&runs, "novel-1"));
        assert!(runs.lock().expect("runs").is_empty());

        runs.lock().expect("runs").insert(
            "novel-1".to_string(),
            AutoRunControl {
                status: "paused".to_string(),
                completed_batches: 1,
                job_id: None,
                profile_ids: HashSet::new(),
            },
        );
        drop(AutoRunCleanup::new(&runs, "novel-1"));
        assert!(runs.lock().expect("runs").contains_key("novel-1"));
    }

    #[test]
    fn terminating_a_paused_run_must_finish_immediately() {
        assert!(should_terminate_paused_run("paused", "terminate_requested"));
        assert!(!should_terminate_paused_run("running", "terminate_requested"));
        assert!(!should_terminate_paused_run("paused", "pause_requested"));
    }
}
