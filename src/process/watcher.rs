// @group BusinessLogic : File system watcher — restarts process on file changes

use anyhow::Result;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    relay_handle: tokio::task::JoinHandle<()>,
}

impl FileWatcher {
    /// Start watching the given paths. When a change is detected (debounced 500ms),
    /// sends the process_id to the provided channel so the manager can restart it.
    pub fn start(
        process_id: Uuid,
        watch_paths: &[String],
        ignore_patterns: &[String],
        restart_tx: mpsc::Sender<Uuid>,
    ) -> Result<Self> {
        let (std_tx, std_rx) = std_mpsc::sync_channel::<notify::Result<Event>>(32);
        let ignore = ignore_patterns.to_vec();
        let rtx = restart_tx.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res| {
                if let Err(error) = std_tx.try_send(res) {
                    match error {
                        std_mpsc::TrySendError::Full(_) => {
                            tracing::debug!(
                                "file watcher event coalesced because the relay is full"
                            )
                        }
                        std_mpsc::TrySendError::Disconnected(_) => {
                            tracing::debug!("file watcher relay is already closed")
                        }
                    }
                }
            },
            Config::default().with_poll_interval(Duration::from_millis(500)),
        )?;

        for path_str in watch_paths {
            let path = Path::new(path_str);
            if !path.exists() {
                anyhow::bail!("watch path does not exist: {}", path.display());
            }
            watcher.watch(path, RecursiveMode::Recursive)?;
        }

        // Spawn a blocking thread to relay events into the async world
        let relay_handle = tokio::task::spawn_blocking(move || {
            let mut last_restart = std::time::Instant::now();
            let debounce = Duration::from_millis(500);

            for result in std_rx {
                let event = match result {
                    Ok(event) => event,
                    Err(error) => {
                        tracing::warn!(%error, "file watcher reported an error");
                        continue;
                    }
                };
                if !event.kind.is_access() {
                    let path_match = event.paths.iter().any(|p| {
                        let name = p.to_string_lossy();
                        !ignore.iter().any(|ig| name.contains(ig.as_str()))
                    });

                    if path_match && last_restart.elapsed() >= debounce {
                        last_restart = std::time::Instant::now();
                        if rtx.blocking_send(process_id).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            _watcher: watcher,
            relay_handle,
        })
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.relay_handle.abort();
    }
}
