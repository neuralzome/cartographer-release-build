//! `crb clean`, and the first thing every build does: the prefix, the work
//! directory, and this platform's tarballs. Other platforms' tarballs in the
//! same out directory are left alone, and so is anything else in it — a
//! mistyped --out must never cost more than two files.

use anyhow::{anyhow, Result};

use crate::Context;

pub fn run(context: &Context) -> Result<()> {
    println!("\n══ clean");
    let mut removed = 0;
    let prefix = context.layout.prefix();
    if prefix.exists() {
        // /opt is root's: removing the prefix itself needs sudo even when
        // everything in it is ours.
        let prefix_text = prefix.to_str().ok_or_else(|| anyhow!("prefix is not UTF-8"))?;
        context.runner.run("clean", "sudo", &["rm", "-rf", prefix_text])?;
        println!("   ✓ removed {}", prefix.display());
        removed += 1;
    }

    let work = context.layout.work_dir();
    if work.exists() {
        context.runner.remove_dir(&work)?;
        println!("   ✓ removed {}", context.layout.relative(&work));
        removed += 1;
    }

    for tarball in context.platform.tarballs() {
        for name in [tarball.clone(), format!("{tarball}.sha256")] {
            let path = context.out.join(&name);
            if path.exists() {
                context.runner.remove_file(&path)?;
                println!("   ✓ removed {}", context.layout.relative(&path));
                removed += 1;
            }
        }
    }
    if removed == 0 {
        println!("   ✓ nothing to remove");
    }
    Ok(())
}
