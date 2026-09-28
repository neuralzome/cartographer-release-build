//! The tarballs, from the builds' install manifests. Each holds exactly what
//! its own builds installed, relative to the prefix, so install.sh extracts
//! both with `tar -xzf … -C /opt/carto`.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, bail, Context as _, Result};

use crate::Context;

/// `builds` by name (their install_manifest.txt), plus `extra` paths relative
/// to the prefix. Writes the tarball and its .sha256 into the out directory.
pub fn pack(context: &Context, tarball: &str, builds: &[&str], extra: &[&str]) -> Result<()> {
    let prefix = context.layout.prefix();
    let target = context.out.join(tarball);
    let list = context.layout.work_dir().join(format!("{tarball}.list"));

    if context.runner.dry_run {
        let sources: Vec<&str> = builds.iter().chain(extra).copied().collect();
        println!("   pack {} from {}", target.display(), sources.join(", "));
        println!("   write {}.sha256", target.display());
        return Ok(());
    }

    let mut entries = Vec::new();
    for build in builds {
        let manifest = context.layout.build_dir(build).join("install_manifest.txt");
        let text = fs::read_to_string(&manifest).with_context(|| format!("reading {}", manifest.display()))?;
        entries.extend(manifest_entries(&text, &prefix)?);
    }
    entries.extend(extra.iter().map(|path| path.to_string()));
    fs::write(&list, entries.join("\n") + "\n").with_context(|| format!("writing {}", list.display()))?;

    let target_text = target.to_str().ok_or_else(|| anyhow!("out directory is not UTF-8"))?;
    let prefix_text = prefix.to_str().ok_or_else(|| anyhow!("prefix is not UTF-8"))?;
    let list_text = list.to_str().ok_or_else(|| anyhow!("work directory is not UTF-8"))?;
    context.runner.run("pack", "tar", &["-czf", target_text, "-C", prefix_text, "-T", list_text])?;

    write_checksum(&context.out, tarball)?;
    let size = fs::metadata(&target).map(|metadata| metadata.len()).unwrap_or(0);
    println!("   ✓ {} ({} files, {:.1} MB)", target.display(), entries.len(), size as f64 / 1e6);
    Ok(())
}

/// `sha256sum`'s own format, run from the out directory so the file names
/// no path and `sha256sum -c` works wherever the pair is downloaded to.
fn write_checksum(out: &Path, tarball: &str) -> Result<()> {
    let output = Command::new("sha256sum")
        .arg(tarball)
        .current_dir(out)
        .output()
        .context("running sha256sum")?;
    if !output.status.success() {
        bail!("sha256sum {tarball} failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let path = out.join(format!("{tarball}.sha256"));
    fs::write(&path, &output.stdout).with_context(|| format!("writing {}", path.display()))
}

/// install_manifest.txt lists absolute paths, one per line. Every one must be
/// under the prefix: anything else would be installed somewhere the tarball
/// cannot put it back.
pub fn manifest_entries(text: &str, prefix: &Path) -> Result<Vec<String>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            Path::new(line)
                .strip_prefix(prefix)
                .map(|relative| relative.to_string_lossy().to_string())
                .map_err(|_| anyhow!("{line} was installed outside {}", prefix.display()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_paths_become_relative() {
        let text = "/opt/carto/lib/libceres.a\n/opt/carto/include/ceres/ceres.h\n\n";
        let entries = manifest_entries(text, Path::new("/opt/carto")).unwrap();
        assert_eq!(entries, ["lib/libceres.a", "include/ceres/ceres.h"]);
    }

    #[test]
    fn a_path_outside_the_prefix_is_an_error() {
        // /opt/carto-old shares the prefix's characters but not its directory.
        assert!(manifest_entries("/usr/local/lib/libceres.a\n", Path::new("/opt/carto")).is_err());
        assert!(manifest_entries("/opt/carto-old/lib/x.a\n", Path::new("/opt/carto")).is_err());
    }
}
