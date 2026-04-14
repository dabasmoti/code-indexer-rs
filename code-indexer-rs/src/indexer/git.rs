use anyhow::{Context, Result};
use git2::Repository;
use std::path::{Path, PathBuf};

pub struct GitDetector {
    repo: Option<Repository>,
}

impl GitDetector {
    pub fn open(path: &Path) -> Self {
        let repo = Repository::discover(path).ok();
        Self { repo }
    }

    pub fn is_git_repo(&self) -> bool {
        self.repo.is_some()
    }

    pub fn head_commit_hash(&self) -> Option<String> {
        let repo = self.repo.as_ref()?;
        let head = repo.head().ok()?;
        let commit = head.peel_to_commit().ok()?;
        Some(commit.id().to_string())
    }

    pub fn changed_files_since(&self, old_commit_hash: &str) -> Result<Vec<PathBuf>> {
        let repo = self.repo.as_ref().context("Not a git repository")?;

        let old_oid = git2::Oid::from_str(old_commit_hash)
            .with_context(|| format!("Invalid commit hash: {}", old_commit_hash))?;

        let old_commit = repo
            .find_commit(old_oid)
            .with_context(|| format!("Commit not found: {}", old_commit_hash))?;

        let old_tree = old_commit
            .tree()
            .context("Failed to get tree from old commit")?;

        let head = repo.head().context("Failed to get HEAD")?;
        let new_commit = head
            .peel_to_commit()
            .context("Failed to peel HEAD to commit")?;
        let new_tree = new_commit
            .tree()
            .context("Failed to get tree from HEAD commit")?;

        let diff = repo
            .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)
            .context("Failed to compute diff between trees")?;

        let workdir = repo
            .workdir()
            .context("Bare repositories are not supported")?
            .to_path_buf();

        let mut changed_files = Vec::new();

        diff.foreach(
            &mut |delta, _progress| {
                if let Some(path) = delta.new_file().path() {
                    changed_files.push(workdir.join(path));
                } else if let Some(path) = delta.old_file().path() {
                    changed_files.push(workdir.join(path));
                }
                true
            },
            None,
            None,
            None,
        )
        .context("Failed to iterate over diff")?;

        Ok(changed_files)
    }
}
