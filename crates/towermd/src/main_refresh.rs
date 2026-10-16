use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::task::JoinHandle;
use towermd_core::{catalog, origin, repo};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MainStatus {
    pub reference: String,
    pub fetched_at: Option<u64>,
    pub last_fetch_error: Option<String>,
}

#[derive(Default)]
struct FetchState {
    last_started: Option<Instant>,
    in_flight: bool,
    fetched_at: Option<u64>,
    last_fetch_error: Option<String>,
    job: Option<JoinHandle<()>>,
}

struct Entry {
    root: Mutex<PathBuf>,
    state: Mutex<FetchState>,
}

#[derive(Default)]
pub(crate) struct MainRefresh {
    stopping: AtomicBool,
    entries: Mutex<BTreeMap<PathBuf, Arc<Entry>>>,
}

fn due(last: Option<Instant>, in_flight: bool, now: Instant) -> bool {
    !in_flight
        && last.is_none_or(|last| now.saturating_duration_since(last) >= Duration::from_secs(60))
}

impl MainRefresh {
    pub(crate) fn observe(&self, directory: &Path) -> Result<MainStatus, String> {
        let root = repo::toplevel(directory)?;
        let common = repo::git(
            &root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let common = PathBuf::from(common)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let reference = catalog::main_reference(&root)?.to_string();
        if reference == "refs/heads/main" {
            return Ok(MainStatus {
                reference,
                fetched_at: None,
                last_fetch_error: None,
            });
        }
        let entry = {
            let mut entries = self.entries.lock().map_err(|_| "main 刷新注册表锁损坏")?;
            entries
                .entry(common)
                .or_insert_with(|| {
                    Arc::new(Entry {
                        root: Mutex::new(root.clone()),
                        state: Mutex::new(FetchState::default()),
                    })
                })
                .clone()
        };
        *entry.root.lock().map_err(|_| "main 刷新路径锁损坏")? = root;
        self.schedule(&entry)?;
        let state = entry.state.lock().map_err(|_| "main 刷新状态锁损坏")?;
        Ok(MainStatus {
            reference,
            fetched_at: state.fetched_at,
            last_fetch_error: state.last_fetch_error.clone(),
        })
    }

    fn schedule(&self, entry: &Arc<Entry>) -> Result<(), String> {
        let root = entry
            .root
            .lock()
            .map_err(|_| "main 刷新路径锁损坏")?
            .clone();
        let mut state = entry.state.lock().map_err(|_| "main 刷新状态锁损坏")?;
        let now = Instant::now();
        if self.stopping.load(Ordering::Acquire) || !due(state.last_started, state.in_flight, now) {
            return Ok(());
        }
        state.last_started = Some(now);
        state.in_flight = true;
        let worker = entry.clone();
        state.job = Some(tokio::task::spawn_blocking(move || {
            let result = origin::refresh_origin_main(&root).and_then(|sha| {
                if sha.is_none() {
                    return Ok(None);
                }
                let millis = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|error| error.to_string())?
                    .as_millis();
                Ok(Some(u64::try_from(millis).map_err(|_| "fetch 时间溢出")?))
            });
            if let Ok(mut state) = worker.state.lock() {
                state.in_flight = false;
                match result {
                    Ok(Some(at)) => {
                        state.fetched_at = Some(at);
                        state.last_fetch_error = None;
                    }
                    Ok(None) => {
                        state.fetched_at = None;
                        state.last_fetch_error = None;
                    }
                    Err(error) => {
                        eprintln!(
                            "towermd 警告：后台 fetch 失败（{}）：{error}",
                            root.display()
                        );
                        state.last_fetch_error = Some(error);
                    }
                }
            }
        }));
        Ok(())
    }

    pub(crate) fn tick(&self) -> Result<(), String> {
        let entries: Vec<_> = self
            .entries
            .lock()
            .map_err(|_| "main 刷新注册表锁损坏")?
            .values()
            .cloned()
            .collect();
        for entry in entries {
            self.schedule(&entry)?;
        }
        Ok(())
    }

    pub(crate) async fn drain(&self) -> Result<(), String> {
        self.stopping.store(true, Ordering::Release);
        let jobs: Vec<_> = {
            let entries = self.entries.lock().map_err(|_| "main 刷新注册表锁损坏")?;
            let mut jobs = Vec::new();
            for entry in entries.values() {
                if let Some(job) = entry
                    .state
                    .lock()
                    .map_err(|_| "main 刷新状态锁损坏")?
                    .job
                    .take()
                {
                    jobs.push(job);
                }
            }
            jobs
        };
        for job in jobs {
            job.await.map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_attempts_are_rate_limited_and_inflight_never_overlaps() {
        let start = Instant::now();
        assert!(due(None, false, start));
        assert!(!due(Some(start), false, start + Duration::from_secs(59)));
        assert!(due(Some(start), false, start + Duration::from_secs(60)));
        assert!(!due(Some(start), true, start + Duration::from_secs(120)));
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fetch_observations_are_rate_limited_retained_and_rebuilt() {
        use std::fs;
        use std::process::Command;
        fn git(root: &Path, args: &[&str]) -> String {
            let output = Command::new("git")
                .current_dir(root)
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            String::from_utf8(output.stdout)
                .unwrap()
                .trim_end_matches('\n')
                .into()
        }
        async fn settled(entry: &Entry) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while entry.state.lock().unwrap().in_flight {
                assert!(Instant::now() < deadline, "fetch did not complete");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "-qb", "main"]);
        towermd_core::init::init(&root, &Default::default()).unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "fixture"]);
        let remote = temp.path().join("remote.git");
        git(&root, &["clone", "--bare", ".", remote.to_str().unwrap()]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        let refresh = MainRefresh::default();
        refresh.observe(&root).unwrap();
        let entry = refresh
            .entries
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        settled(&entry).await;
        let successful = refresh.observe(&root).unwrap();
        let fetched = successful.fetched_at.unwrap();
        assert!(fetched > 0);
        assert!(successful.last_fetch_error.is_none());
        assert!(!root.join(".git/towermd").exists());
        let worktree = temp.path().join("linked");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-qb",
                "feature",
                worktree.to_str().unwrap(),
            ],
        );
        assert_eq!(
            refresh.observe(&worktree).unwrap().fetched_at,
            Some(fetched)
        );
        assert_eq!(refresh.entries.lock().unwrap().len(), 1);
        git(
            &root,
            &["remote", "set-url", "origin", "/nonexistent/main-refresh"],
        );
        entry.state.lock().unwrap().last_started = Some(Instant::now() - Duration::from_secs(60));
        refresh.observe(&root).unwrap();
        settled(&entry).await;
        let failed = refresh.observe(&root).unwrap();
        assert_eq!(failed.fetched_at, Some(fetched));
        assert!(failed.last_fetch_error.is_some());
        let last = entry.state.lock().unwrap().last_started;
        refresh.observe(&worktree).unwrap();
        assert_eq!(entry.state.lock().unwrap().last_started, last);
        let rebuilt = MainRefresh::default();
        assert!(rebuilt.observe(&root).unwrap().fetched_at.is_none());
        assert_eq!(
            towermd_core::catalog::Catalog::load(&root)
                .unwrap()
                .revision,
            git(&remote, &["rev-parse", "main"])
        );
        rebuilt.drain().await.unwrap();
        git(
            &root,
            &["remote", "set-url", "origin", remote.to_str().unwrap()],
        );
        entry.state.lock().unwrap().last_started = Some(Instant::now() - Duration::from_secs(60));
        refresh.observe(&root).unwrap();
        settled(&entry).await;
        let recovered = refresh.observe(&root).unwrap();
        assert!(recovered.fetched_at.unwrap() >= fetched);
        assert!(recovered.last_fetch_error.is_none());
        git(&root, &["remote", "remove", "origin"]);
        let local = refresh.observe(&root).unwrap();
        assert_eq!(local.reference, "refs/heads/main");
        assert!(local.fetched_at.is_none());
        assert!(local.last_fetch_error.is_none());
        refresh.drain().await.unwrap();
    }
}
