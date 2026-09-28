//! Where things are.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

/// This tool's own manifest. It lives at the root of the cartographer-build
/// repository and at cartographer/ in the ubr monorepo, so it is found by
/// what it says rather than by where it is.
const MANIFEST: &str = "Cargo.toml";
const PACKAGE_LINE: &str = "name = \"crb\"";

/// Where everything is built and installed, and where install.sh extracts the
/// tarballs. Fixed, because the CMake config files installed there name it.
pub const PREFIX: &str = "/opt/carto";

/// Where crb is installed, for the staleness check.
pub const INSTALLED: &str = "/usr/local/bin/crb";

pub struct Layout {
    /// The directory holding crb's Cargo.toml.
    pub root: PathBuf,
}

impl Layout {
    /// Walks up from `start` looking for crb's manifest; from the ubr
    /// monorepo's root, cartographer/ is looked in too.
    pub fn discover(start: &Path) -> Result<Self> {
        let mut candidate = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
        loop {
            for directory in [candidate.clone(), candidate.join("cartographer")] {
                if is_crb(&directory) {
                    return Ok(Layout { root: directory });
                }
            }
            if !candidate.pop() {
                return Err(anyhow!("not inside a crb checkout (no crb {MANIFEST} above {})", start.display()));
            }
        }
    }

    /// Repo-relative where possible, so output stays readable.
    pub fn relative(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(relative) => relative.to_string_lossy().to_string(),
            Err(_) => path.to_string_lossy().to_string(),
        }
    }

    pub fn tool_dir(&self) -> PathBuf {
        self.root.clone()
    }

    /// Sources, build trees and logs. Gitignored, removed by `clean` and at the
    /// start of every build, and left in place when a build fails.
    pub fn work_dir(&self) -> PathBuf {
        self.tool_dir().join("build")
    }

    pub fn source_dir(&self, name: &str) -> PathBuf {
        self.work_dir().join(name)
    }

    pub fn build_dir(&self, name: &str) -> PathBuf {
        self.work_dir().join(format!("{name}-build"))
    }

    pub fn log_dir(&self) -> PathBuf {
        self.work_dir().join("logs")
    }

    /// The tarballs, unless --out says otherwise. Gitignored.
    pub fn default_out(&self) -> PathBuf {
        self.tool_dir().join("dist")
    }

    /// This tool's own sources, for the staleness check.
    pub fn crb_src(&self) -> PathBuf {
        self.tool_dir().join("src")
    }

    pub fn prefix(&self) -> PathBuf {
        PathBuf::from(PREFIX)
    }
}

fn is_crb(directory: &Path) -> bool {
    std::fs::read_to_string(directory.join(MANIFEST))
        .map(|text| text.lines().any(|line| line.trim() == PACKAGE_LINE))
        .unwrap_or(false)
}
