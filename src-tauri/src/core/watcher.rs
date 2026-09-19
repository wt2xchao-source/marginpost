use super::ports::{ChangeDetector, CoreResult};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

pub struct LocalChangeDetector;

impl ChangeDetector for LocalChangeDetector {
    fn wait_for_change(&self, root: &Path, timeout: Duration) -> CoreResult<PathBuf> {
        let (sender, receiver) = mpsc::channel();
        let mut watcher: RecommendedWatcher = notify::recommended_watcher(sender)?;
        watcher.watch(root, RecursiveMode::Recursive)?;

        loop {
            let event = receiver.recv_timeout(timeout)??;
            if let Some(path) = event.paths.into_iter().find(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            }) {
                return Ok(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;

    #[test]
    fn detects_a_markdown_file_change() {
        let directory = tempfile::tempdir().expect("temp directory");
        let target = directory.path().join("watched.md");
        fs::write(&target, "# Initial").expect("write initial file");
        let writer_target = target.clone();

        let writer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            fs::write(writer_target, "# Changed").expect("write watched file");
        });

        let detected = LocalChangeDetector
            .wait_for_change(directory.path(), Duration::from_secs(5))
            .expect("detect markdown change");
        writer.join().expect("writer thread");

        assert_eq!(
            detected.canonicalize().expect("canonical detected path"),
            target.canonicalize().expect("canonical target path")
        );
    }
}
