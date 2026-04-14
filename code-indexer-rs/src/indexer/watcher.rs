use anyhow::{Context, Result};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

pub struct FileWatcher {
    debounce_ms: u64,
}

impl FileWatcher {
    pub fn new(debounce_ms: u64) -> Self {
        Self { debounce_ms }
    }

    pub fn watch<F>(&self, path: &Path, mut callback: F) -> Result<()>
    where
        F: FnMut(Vec<std::path::PathBuf>) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<DebounceEventResult>();

        let debounce_duration = Duration::from_millis(self.debounce_ms);

        let mut debouncer: Debouncer<notify::RecommendedWatcher> =
            new_debouncer(debounce_duration, tx)
                .context("Failed to create file watcher debouncer")?;

        debouncer
            .watcher()
            .watch(path, notify::RecursiveMode::Recursive)
            .with_context(|| format!("Failed to watch path: {}", path.display()))?;

        std::thread::spawn(move || {
            // Keep debouncer alive on this thread
            let _debouncer = debouncer;

            for result in rx {
                match result {
                    Ok(events) => {
                        let paths: Vec<std::path::PathBuf> =
                            events.into_iter().map(|e| e.path).collect();
                        if !paths.is_empty() {
                            callback(paths);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(())
    }
}
