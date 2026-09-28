//! `crb verify`: the installation in the prefix and this platform's tarballs.
//! Read-only, and indifferent to how the prefix was filled — after a build
//! or after install.sh extracted the release — so it answers "is this
//! machine's Cartographer the one the pins say, and does it run". See README
//! "What verify proves".

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::pins::{self, Pin, ABSEIL, CARTOGRAPHER, CERES, PROTOBUF};
use crate::report::{Finding, Report};
use crate::runner::query;
use crate::Context;

/// Library and CMake config per build: what pycarto's CMakeLists finds.
const INSTALLED: [(&str, &str, &str); 4] = [
    ("abseil", "lib/libabsl_base.a", "lib/cmake/absl/abslConfig.cmake"),
    ("protobuf", "lib/libprotobuf.a", "lib/cmake/protobuf/protobuf-config.cmake"),
    ("Ceres", "lib/libceres.a", "lib/cmake/Ceres/CeresConfig.cmake"),
    ("Cartographer", "lib/libcartographer.a", "share/cartographer/cartographer-config.cmake"),
];

const CONFIGURATION_FILES: &str = "share/cartographer/configuration_files";

pub fn run(context: &Context) -> Report {
    let mut report = Report::new();
    let prefix = context.layout.prefix();
    version(context, &prefix, &mut report);
    for (name, library, config) in INSTALLED {
        report.push(installed(&prefix, name, library, config));
    }
    report.push(protoc(&prefix));
    report.push(configuration_files(&prefix));
    report.push(print_configuration(&prefix));
    tarballs(context, &mut report);
    report
}

/// VERSION against the pins this crb carries, key by key.
fn version(context: &Context, prefix: &Path, report: &mut Report) {
    let path = prefix.join("VERSION");
    let Ok(text) = fs::read_to_string(&path) else {
        report.push(Finding::fail(
            format!("no {}", path.display()),
            "Nothing is installed there, or not by this tool. `crb build` makes it.",
        ));
        return;
    };
    let recorded = pins::parse_version_file(&text);

    let tag = context.platform.tag();
    match recorded.get("platform") {
        Some(platform) if *platform == tag => report.push(Finding::pass(format!("built for {tag}"))),
        other => report.push(Finding::fail(
            format!("built for {}, this machine is {tag}", other.map_or("nothing recorded", String::as_str)),
            "Static libraries from another distro link against libraries this one does not have.",
        )),
    }
    for pin in [&CARTOGRAPHER, &CERES, &PROTOBUF, &ABSEIL] {
        report.push(pinned(pin, recorded.get(pin.name)));
    }
    match recorded.get("cartographer_cxx_flags_release").map(String::as_str) {
        Some("-O3 -DNDEBUG") => report.push(Finding::pass("Cartographer compiled at -O3")),
        Some(flags) => report.push(Finding::warn(
            format!("Cartographer compiled with {flags}"),
            "A --low-mem build. It works, and localises slower than an -O3 one.",
        )),
        None => report.push(Finding::fail("VERSION records no Cartographer flags", "Rebuild with `crb build`.")),
    }
}

fn pinned(pin: &Pin, recorded: Option<&String>) -> Finding {
    match recorded {
        Some(git_ref) if git_ref == pin.git_ref => Finding::pass(format!("{} {}", pin.name, pin.git_ref)),
        Some(git_ref) => Finding::fail(
            format!("{} is {git_ref}, the pin is {}", pin.name, pin.git_ref),
            "The installation predates the pins in src/pins.rs. Rebuild with `crb build`.",
        ),
        None => Finding::fail(format!("VERSION records no {}", pin.name), "Rebuild with `crb build`."),
    }
}

fn installed(prefix: &Path, name: &str, library: &str, config: &str) -> Finding {
    let missing: Vec<&str> = [library, config].into_iter().filter(|path| !prefix.join(path).is_file()).collect();
    if missing.is_empty() {
        Finding::pass(format!("{name}: {library} and its CMake config"))
    } else {
        Finding::fail(
            format!("{name}: missing {}", missing.join(", ")),
            "pycarto's CMake would not find it.",
        )
    }
}

/// The protoc Cartographer's .pb.h files were generated with. Not the one on
/// PATH: that is ubr's 27.2, which must never have been used here.
fn protoc(prefix: &Path) -> Finding {
    let protoc = prefix.join("bin/protoc");
    let wanted = pins::protoc_version();
    match query(&protoc.to_string_lossy(), &["--version"]) {
        Some(version) if version.trim() == wanted => Finding::pass(format!("{} reports {wanted}", protoc.display())),
        Some(version) => Finding::fail(
            format!("{} reports {}, expected {wanted}", protoc.display(), version.trim()),
            "The prefix holds a different protobuf from the one Cartographer was pinned to.",
        ),
        None => Finding::fail(format!("{} does not run", protoc.display()), "protobuf is missing or broken."),
    }
}

/// Read when a MapBuilder is made, not when anything is compiled: a prefix
/// without them builds pycarto fine and fails at the first map.
fn configuration_files(prefix: &Path) -> Finding {
    let directory = prefix.join(CONFIGURATION_FILES);
    let missing: Vec<&str> = ["map_builder.lua", "trajectory_builder.lua", "trajectory_builder_2d.lua", "pose_graph.lua"]
        .into_iter()
        .filter(|name| !directory.join(name).is_file())
        .collect();
    if missing.is_empty() {
        Finding::pass(format!("Lua configuration files in {}", directory.display()))
    } else {
        Finding::fail(
            format!("missing from {}: {}", directory.display(), missing.join(", ")),
            "Every Lua configuration includes these.",
        )
    }
}

/// A binary linked against every library above, resolving the same includes
/// a MapBuilder's configuration will. It running is the proof the pieces fit.
fn print_configuration(prefix: &Path) -> Finding {
    let tool = prefix.join("bin/cartographer_print_configuration");
    let directory = prefix.join(CONFIGURATION_FILES);
    let output = Command::new(&tool)
        .arg(format!("--configuration_directories={}", directory.display()))
        .arg("--configuration_basename=trajectory_builder.lua")
        .output();
    match output {
        Ok(output) if output.status.success() => {
            Finding::pass("cartographer_print_configuration runs and loads trajectory_builder.lua")
        }
        Ok(output) => Finding::fail(
            "cartographer_print_configuration failed on trajectory_builder.lua",
            String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or("no output").to_string(),
        ),
        Err(error) => Finding::fail(format!("cannot run {}", tool.display()), error.to_string()),
    }
}

/// This platform's two tarballs: present, matching their .sha256, and holding
/// what each should. Skipped, not failed, when there are none: a machine
/// that installed the release has a prefix and no tarballs, legitimately.
fn tarballs(context: &Context, report: &mut Report) {
    let wanted: [(String, &[&str]); 2] = [
        (context.platform.dependencies_tarball(), &["lib/libabsl_base.a", "lib/libprotobuf.a", "bin/protoc", "lib/libceres.a"]),
        (context.platform.cartographer_tarball(), &["lib/libcartographer.a", "VERSION"]),
    ];
    for (tarball, holds) in wanted {
        let path = context.out.join(&tarball);
        if !path.is_file() {
            report.push(Finding::skip(
                format!("no {}", context.layout.relative(&path)),
                "Nothing to check. `crb build` makes it.",
            ));
            continue;
        }

        let checksum = Command::new("sha256sum")
            .args(["-c", &format!("{tarball}.sha256")])
            .current_dir(&context.out)
            .output();
        match checksum {
            Ok(output) if output.status.success() => report.push(Finding::pass(format!("{tarball} matches its .sha256"))),
            _ => report.push(Finding::fail(
                format!("{tarball} does not match its .sha256"),
                "Changed after it was packed, or the .sha256 is missing. Rebuild before uploading.",
            )),
        }

        let listing = query("tar", &["-tzf", &path.to_string_lossy()]).unwrap_or_default();
        let missing: Vec<&str> = holds.iter().copied().filter(|entry| !listing.lines().any(|line| line == *entry)).collect();
        if missing.is_empty() {
            report.push(Finding::pass(format!("{tarball} holds {}", holds.join(", "))));
        } else {
            report.push(Finding::fail(
                format!("{tarball} is missing {}", missing.join(", ")),
                "It was packed from an incomplete build.",
            ));
        }
    }
}
