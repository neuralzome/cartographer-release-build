//! `crb doctor`: every precondition, read-only. `build` runs the same set and
//! refuses on a failure, because each of these otherwise fails deep into an
//! hour-long build. See README "Preconditions".

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use crate::build::Options;
use crate::host;
use crate::layout::INSTALLED;
use crate::platform::{TARGET_ID, TARGET_VERSION_ID};
use crate::report::{Finding, Report};
use crate::runner::{on_path, query};
use crate::Context;

/// Programs the build runs.
const TOOLS: [&str; 8] = ["git", "cmake", "ninja", "g++", "pkg-config", "tar", "sha256sum", "sudo"];

/// What Cartographer and Ceres build against, from apt. Kept in step with
/// install-prereqs.sh, which installs them. gmock and gtest are for
/// Cartographer's configure step, which finds them unconditionally
/// (google_enable_testing) even though no test target is built.
pub const APT_PACKAGES: [&str; 11] = [
    "libboost-iostreams-dev",
    "libcairo2-dev",
    "libeigen3-dev",
    "libgflags-dev",
    "libgoogle-glog-dev",
    "liblua5.3-dev",
    "liblapack-dev",
    "libblas-dev",
    "zlib1g-dev",
    "libgmock-dev",
    "libgtest-dev",
];

pub fn run(context: &Context, options: &Options) -> Report {
    let mut report = Report::new();
    report.push(platform(context));
    report.push(tools());
    report.push(apt_packages());
    report.push(memory(options));
    report.push(disk(context));
    report.push(sudo(context));
    binary_freshness(context, &mut report);
    report
}

/// Static libraries are only good on the distro they were built on, and ubr
/// runs on Ubuntu 22.04. Anything else still builds, under its own name.
fn platform(context: &Context) -> Finding {
    let tag = context.platform.tag();
    if context.platform.is_target() {
        Finding::pass(format!("platform {tag}"))
    } else {
        Finding::warn(
            format!("platform {tag} is not {TARGET_ID} {TARGET_VERSION_ID}"),
            format!(
                "The tarballs will be named {tag} and are no use to ubr, which runs {TARGET_ID} {TARGET_VERSION_ID}\n\
                 on the laptop's container and the Orin. Build in the ubr container instead."
            ),
        )
    }
}

fn tools() -> Finding {
    let missing: Vec<&str> = TOOLS.iter().copied().filter(|tool| !on_path(tool)).collect();
    if missing.is_empty() {
        Finding::pass(format!("tools on PATH: {}", TOOLS.join(", ")))
    } else {
        Finding::fail(
            format!("not on PATH: {}", missing.join(", ")),
            "Run install-prereqs.sh.",
        )
    }
}

fn apt_packages() -> Finding {
    if !on_path("dpkg-query") {
        return Finding::skip("cannot check the apt packages", "No dpkg-query: not a Debian or Ubuntu machine.");
    }
    let missing: Vec<&str> = APT_PACKAGES
        .iter()
        .copied()
        .filter(|package| {
            query("dpkg-query", &["-W", "-f=${Status}", package])
                .map(|status| status.trim() != "install ok installed")
                .unwrap_or(true)
        })
        .collect();
    if missing.is_empty() {
        Finding::pass(format!("{} apt packages Cartographer builds against", APT_PACKAGES.len()))
    } else {
        Finding::fail(
            format!("apt packages missing: {}", missing.join(", ")),
            "Ceres or Cartographer would fail to configure. Run install-prereqs.sh.",
        )
    }
}

/// Each Cartographer compile takes 3-5 GB at -O3; too many at once and the
/// build is killed for memory an hour in, with nothing but "Killed" to show.
fn memory(options: &Options) -> Finding {
    let Some(memory_gb) = host::memory_gb() else {
        return Finding::skip("cannot read /proc/meminfo", "No memory check.");
    };
    let needed = options.jobs as f64 * host::GB_PER_BUILD_JOB;
    let fits = host::jobs_that_fit(memory_gb);
    if needed <= memory_gb {
        Finding::pass(format!("{} job(s) need about {needed:.0} GB; this machine has {memory_gb:.0}", options.jobs))
    } else if options.low_mem {
        Finding::warn(
            format!("{} job(s) with --low-mem on {memory_gb:.0} GB", options.jobs),
            format!("-O1 needs less than the {needed:.0} GB this assumes, but not predictably less. --jobs {fits} is safe."),
        )
    } else {
        Finding::warn(
            format!("{} job(s) need about {needed:.0} GB; this machine has {memory_gb:.0}", options.jobs),
            format!(
                "The compiler may be killed for memory partway through. --jobs {fits} fits;\n\
                 --low-mem trades a slower Cartographer for less memory per job."
            ),
        )
    }
}

fn disk(context: &Context) -> Finding {
    let work = context.layout.work_dir();
    match host::free_gb(&work) {
        Some(free) if free >= host::MIN_FREE_GB => {
            Finding::pass(format!("{free:.0} GB free for {}", context.layout.relative(&work)))
        }
        Some(free) => Finding::fail(
            format!("{free:.0} GB free for {}; the build needs about {:.0}", context.layout.relative(&work), host::MIN_FREE_GB),
            "Four source trees and their build trees. Free some space first.",
        ),
        None => Finding::skip("cannot read the free space", "No disk check."),
    }
}

/// The prefix is under /opt, which is root's: making it and removing it take
/// sudo. Nothing else does.
fn sudo(context: &Context) -> Finding {
    if query("sudo", &["-n", "true"]).is_some() {
        Finding::pass("sudo works without a prompt")
    } else {
        Finding::warn(
            "sudo will ask for a password",
            format!(
                "Only to create and remove {}. The build waits at the prompt, so stay for its first minute.",
                context.layout.prefix().display()
            ),
        )
    }
}

/// Whether the crb being run was built from the sources as they stand, and
/// whether the copy on PATH is that one. As msd's: a stale binary builds with
/// older pins than the tree says, and says nothing.
fn binary_freshness(context: &Context, report: &mut Report) {
    let Some(newest) = newest_modification(&context.layout.crb_src()) else {
        report.push(Finding::skip("cannot read crb's own sources", "No staleness check."));
        return;
    };
    let Ok(current) = std::env::current_exe() else {
        report.push(Finding::skip("cannot locate the running crb", "No staleness check."));
        return;
    };
    match modified(&current) {
        Some(built) if built < newest => report.push(Finding::warn(
            "the crb you are running is older than its sources",
            "It was built before the last edit to src/, so its pins may not be the tree's.\n\
             Run install.sh.",
        )),
        Some(_) => report.push(Finding::pass("crb is built from the current sources")),
        None => report.push(Finding::skip("cannot stat the running crb", "No staleness check.")),
    }

    let installed = Path::new(INSTALLED);
    match (modified(installed), modified(&current)) {
        (None, _) => report.push(Finding::warn(
            format!("crb is not installed at {INSTALLED}"),
            "It runs from wherever it was built. install.sh builds and installs it.",
        )),
        (Some(there), Some(here)) if there < here => report.push(Finding::warn(
            format!("{INSTALLED} is older than the one you are running"),
            "A shell resolving `crb` from PATH would get the older one. Re-run install.sh.",
        )),
        (Some(_), _) => report.push(Finding::pass(format!("{INSTALLED} is up to date"))),
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

fn newest_modification(directory: &Path) -> Option<SystemTime> {
    fs::read_dir(directory)
        .ok()?
        .filter_map(|entry| modified(&entry.ok()?.path()))
        .max()
}
