//! Local file storage for uploaded PDFs, cropped figures and LaTeX workspaces.
//!
//! The database stores *relative* paths (always with `/` separators) so a whole
//! library can be moved or restored from a backup without rewriting any rows.

use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn new(root: impl Into<PathBuf>) -> AppResult<Self> {
        let root = root.into();
        for sub in ["papers", "images", "latex"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let root = root.canonicalize().unwrap_or(root);
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a stored relative path, refusing anything that could escape the
    /// storage root (absolute paths, `..`, drive prefixes).
    pub fn resolve(&self, relative: &str) -> AppResult<PathBuf> {
        let rel = Path::new(relative);
        let mut out = self.root.clone();
        let mut any = false;
        for component in rel.components() {
            match component {
                Component::Normal(part) => {
                    out.push(part);
                    any = true;
                }
                _ => {
                    return Err(AppError::bad_request(
                        "Stored file path is outside the expected storage directory",
                    ))
                }
            }
        }
        if !any {
            return Err(AppError::bad_request("Stored file path is empty"));
        }
        Ok(out)
    }

    /// Relative `/`-separated form of a path under the storage root.
    pub fn relative(&self, absolute: &Path) -> AppResult<String> {
        let rel = absolute
            .strip_prefix(&self.root)
            .map_err(|_| AppError::internal("Path is outside the storage root"))?;
        Ok(rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"))
    }

    pub fn papers_dir(&self, project_id: i64) -> PathBuf {
        self.root.join("papers").join(project_id.to_string())
    }

    pub fn images_dir(&self, project_id: i64) -> PathBuf {
        self.root.join("images").join(project_id.to_string())
    }

    pub fn latex_dir(&self, project_id: i64) -> PathBuf {
        self.root.join("latex").join(project_id.to_string())
    }

    /// Delete one stored file (missing files are not an error).
    pub fn remove_file(&self, relative: &str) {
        match self.resolve(relative) {
            Ok(path) => {
                if let Err(err) = std::fs::remove_file(&path) {
                    if err.kind() != std::io::ErrorKind::NotFound {
                        tracing::warn!(path = %path.display(), %err, "could not delete stored file");
                    }
                }
            }
            Err(err) => tracing::warn!(%err, relative, "refusing to delete path outside storage"),
        }
    }

    /// Delete a stored directory tree (missing directories are not an error).
    pub fn remove_dir(&self, relative: &str) {
        match self.resolve(relative) {
            Ok(path) => {
                if path != self.root {
                    if let Err(err) = std::fs::remove_dir_all(&path) {
                        if err.kind() != std::io::ErrorKind::NotFound {
                            tracing::warn!(path = %path.display(), %err, "could not delete workspace");
                        }
                    }
                }
            }
            Err(err) => tracing::warn!(%err, relative, "refusing to delete path outside storage"),
        }
    }

    pub fn remove_abs_dir(&self, path: &Path) {
        if path.starts_with(&self.root) && path != self.root {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_rejects_escapes() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).unwrap();
        assert!(storage.resolve("papers/1/a.pdf").is_ok());
        assert!(storage.resolve("../etc/passwd").is_err());
        assert!(storage.resolve("/etc/passwd").is_err());
        assert!(storage.resolve("").is_err());
    }
}
