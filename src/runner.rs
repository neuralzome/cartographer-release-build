//! Every command that changes something, in one place: --dry-run prints it
//! instead, and without --verbose its output goes to a log per step, whose
//! tail is printed when it fails. An hour-long build that fails silently, or
//! buries the error under ten thousand lines of compiler output, is the thing
//! this exists to prevent.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Lines of a failed step's log printed with the error.
const TAIL_LINES: usize = 40;

pub struct Runner {
    pub dry_run: bool,
    pub verbose: bool,
    pub log_dir: PathBuf,
}

impl Runner {
    /// Run `program` for the step `log`, from the current directory.
    pub fn run(&self, log: &str, program: &str, arguments: &[&str]) -> Result<()> {
        self.run_command(log, program, arguments, None)
    }

    /// The same, from `directory`.
    pub fn run_in(&self, log: &str, directory: &Path, program: &str, arguments: &[&str]) -> Result<()> {
        self.run_command(log, program, arguments, Some(directory))
    }

    fn run_command(&self, log: &str, program: &str, arguments: &[&str], directory: Option<&Path>) -> Result<()> {
        let line = display(program, arguments);
        if self.dry_run {
            println!("   $ {line}");
            return Ok(());
        }

        let mut command = Command::new(program);
        command.args(arguments);
        if let Some(directory) = directory {
            command.current_dir(directory);
        }

        if self.verbose {
            println!("   $ {line}");
            let status = command.status().with_context(|| format!("running {line}"))?;
            if !status.success() {
                bail!("{line} failed ({status})");
            }
            return Ok(());
        }

        let log_path = self.log_path(log);
        let mut file = self.open_log(&log_path)?;
        writeln!(file, "$ {line}")?;
        // sudo asks on the terminal, not on stdout, so a password prompt
        // still reaches the user with the output going to the log.
        let status = command
            .stdin(Stdio::inherit())
            .stdout(file.try_clone()?)
            .stderr(file.try_clone()?)
            .status()
            .with_context(|| format!("running {line}"))?;
        if !status.success() {
            bail!(
                "{line} failed ({status})\n\n{}\n\nfull log: {}",
                tail(&log_path, TAIL_LINES),
                log_path.display()
            );
        }
        Ok(())
    }

    /// Write `contents` to `path`, or say that it would.
    pub fn write(&self, path: &Path, contents: &str) -> Result<()> {
        if self.dry_run {
            println!("   write {}", path.display());
            return Ok(());
        }
        fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
    }

    /// Remove a directory this user owns, or say that it would.
    pub fn remove_dir(&self, path: &Path) -> Result<()> {
        if self.dry_run {
            println!("   $ rm -rf {}", path.display());
            return Ok(());
        }
        fs::remove_dir_all(path).with_context(|| format!("removing {}", path.display()))
    }

    pub fn remove_file(&self, path: &Path) -> Result<()> {
        if self.dry_run {
            println!("   $ rm -f {}", path.display());
            return Ok(());
        }
        fs::remove_file(path).with_context(|| format!("removing {}", path.display()))
    }

    pub fn create_dir(&self, path: &Path) -> Result<()> {
        if self.dry_run {
            println!("   $ mkdir -p {}", path.display());
            return Ok(());
        }
        fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))
    }

    fn log_path(&self, log: &str) -> PathBuf {
        self.log_dir.join(format!("{log}.log"))
    }

    fn open_log(&self, path: &Path) -> Result<File> {
        fs::create_dir_all(&self.log_dir).with_context(|| format!("creating {}", self.log_dir.display()))?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening {}", path.display()))
    }
}

/// Read-only: runs even under --dry-run. None when it cannot run or fails.
pub fn query(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

/// Whether `program` is on PATH.
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|directory| directory.join(program).is_file()))
        .unwrap_or(false)
}

/// A command line as it would be typed.
pub fn display(program: &str, arguments: &[&str]) -> String {
    let mut line = program.to_string();
    for argument in arguments {
        line.push(' ');
        if argument.is_empty() || argument.contains([' ', '"', '\'', '$', '*']) {
            line.push_str(&format!("'{}'", argument.replace('\'', r"'\''")));
        } else {
            line.push_str(argument);
        }
    }
    line
}

fn tail(path: &Path, lines: usize) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_quotes_only_what_needs_it() {
        assert_eq!(
            display("cmake", &["-DCMAKE_CXX_FLAGS_RELEASE=-O3 -DNDEBUG", "-G", "Ninja"]),
            "cmake '-DCMAKE_CXX_FLAGS_RELEASE=-O3 -DNDEBUG' -G Ninja"
        );
    }
}
