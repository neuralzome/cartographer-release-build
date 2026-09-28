//! What this machine can give the build: memory for parallel compiles, and
//! disk for the sources and build trees.

use std::fs;
use std::path::Path;
use std::process::Command;

/// A Cartographer translation unit takes 3-5 GB to compile at -O3; this leaves
/// headroom for the rest of the machine. The same figure msd uses.
pub const GB_PER_BUILD_JOB: f64 = 4.5;

/// The four source trees and their build trees, with room to spare.
pub const MIN_FREE_GB: f64 = 15.0;

pub fn memory_gb() -> Option<f64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / (1024.0 * 1024.0))
}

/// Parallel compiles that fit in `memory_gb`, never fewer than one.
pub fn jobs_that_fit(memory_gb: f64) -> u32 {
    ((memory_gb / GB_PER_BUILD_JOB).floor() as u32).max(1)
}

/// Free space, in GB, on the filesystem that holds `path` (or its nearest
/// existing parent). `df` rather than statvfs: one call, no libc binding.
pub fn free_gb(path: &Path) -> Option<f64> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            return None;
        }
    }
    let output = Command::new("df").args(["-Pk"]).arg(&probe).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let available_kb: f64 = text.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()?;
    Some(available_kb / (1024.0 * 1024.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_fit_memory() {
        // A 16 GB Orin NX reports a little under 16.
        assert_eq!(jobs_that_fit(15.3), 3);
        assert_eq!(jobs_that_fit(64.0), 14);
        assert_eq!(jobs_that_fit(2.0), 1);
    }
}
