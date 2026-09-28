//! Which platform a build is for, from the CPU and /etc/os-release.
//!
//! The tarballs are named after it, because static libraries built against one
//! distro's glog, gflags and glibc are only good on that distro. ubr runs on
//! Ubuntu 22.04 everywhere: the laptop's container and the Orin's JetPack 6.

use std::fs;

use anyhow::{anyhow, bail, Context, Result};

/// What ubr's image and the robot run.
pub const TARGET_ID: &str = "ubuntu";
pub const TARGET_VERSION_ID: &str = "22.04";

#[derive(Debug)]
pub struct Platform {
    pub id: String,
    pub version_id: String,
    pub arch: &'static str,
}

impl Platform {
    pub fn detect() -> Result<Platform> {
        // crb is only ever built natively (install.sh), so its own target is
        // the machine's.
        let arch = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            other => bail!("unsupported architecture: {other}"),
        };
        let text = fs::read_to_string("/etc/os-release").context("reading /etc/os-release")?;
        Platform::from_os_release(&text, arch)
    }

    pub fn from_os_release(text: &str, arch: &'static str) -> Result<Platform> {
        let field = |key: &str| -> Result<String> {
            text.lines()
                .find_map(|line| line.strip_prefix(&format!("{key}=")))
                .map(|value| value.trim().trim_matches('"').to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("/etc/os-release has no {key}"))
        };
        Ok(Platform { id: field("ID")?, version_id: field("VERSION_ID")?, arch })
    }

    /// e.g. ubuntu22.04-aarch64.
    pub fn tag(&self) -> String {
        format!("{}{}-{}", self.id, self.version_id, self.arch)
    }

    pub fn is_target(&self) -> bool {
        self.id == TARGET_ID && self.version_id == TARGET_VERSION_ID
    }

    pub fn dependencies_tarball(&self) -> String {
        format!("dependencies-{}.tar.gz", self.tag())
    }

    pub fn cartographer_tarball(&self) -> String {
        format!("cartographer-{}.tar.gz", self.tag())
    }

    pub fn tarballs(&self) -> [String; 2] {
        [self.dependencies_tarball(), self.cartographer_tarball()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_from_ubuntu_os_release() {
        let text = "NAME=\"Ubuntu\"\nVERSION_ID=\"22.04\"\nID=ubuntu\nID_LIKE=debian\n";
        let platform = Platform::from_os_release(text, "aarch64").unwrap();
        assert_eq!(platform.tag(), "ubuntu22.04-aarch64");
        assert!(platform.is_target());
        assert_eq!(platform.cartographer_tarball(), "cartographer-ubuntu22.04-aarch64.tar.gz");
    }

    #[test]
    fn other_distros_are_named_and_not_the_target() {
        let text = "ID=debian\nVERSION_ID=\"12\"\n";
        let platform = Platform::from_os_release(text, "x86_64").unwrap();
        assert_eq!(platform.tag(), "debian12-x86_64");
        assert!(!platform.is_target());
    }

    #[test]
    fn a_missing_version_is_an_error() {
        assert!(Platform::from_os_release("ID=arch\n", "x86_64").is_err());
    }
}
