//! `crb build`: preconditions, clean, the four builds in order, the tarballs,
//! then verify. See README "What build does".

use std::fs;
use std::time::Instant;

use anyhow::{anyhow, bail, Context as _, Result};

use crate::pins::{self, Pin, ABSEIL, CARTOGRAPHER, CERES, PROTOBUF};
use crate::report::Report;
use crate::runner::query;
use crate::{checks, clean, pack, verify, Context};

/// The Cartographer targets built: the library and the four tools its install
/// rules need, not the ~130 test targets, several of which do not compile
/// against current gmock.
const CARTOGRAPHER_TARGETS: [&str; 5] = [
    "cartographer",
    "cartographer_autogenerate_ground_truth",
    "cartographer_compute_relations_metrics",
    "cartographer_pbstream",
    "cartographer_print_configuration",
];

/// CMake's own Release flags, written out so VERSION records them.
const RELEASE_FLAGS: &str = "-O3 -DNDEBUG";
/// py-cartographer's LOW_MEM: -O1, and GCC collecting its garbage early and
/// often. Cartographer only; the others fit at -O3.
const LOW_MEM_FLAGS: &str = "-O1 -DNDEBUG --param ggc-min-expand=10 --param ggc-min-heapsize=32768";

pub struct Options {
    pub jobs: u32,
    pub low_mem: bool,
}

impl Options {
    /// 3 jobs fit a 16 GB Orin NX at -O3; --low-mem uses 2. --jobs wins.
    pub fn resolve(jobs: Option<u32>, low_mem: bool) -> Options {
        Options { jobs: jobs.unwrap_or(if low_mem { 2 } else { 3 }), low_mem }
    }

    pub fn carto_cxx_flags_release(&self) -> &'static str {
        if self.low_mem {
            LOW_MEM_FLAGS
        } else {
            RELEASE_FLAGS
        }
    }
}

pub fn run(context: &Context, options: &Options) -> Result<bool> {
    let preconditions = checks::run(context, options);
    preconditions.print("preconditions");
    if preconditions.failed() {
        println!("\nNothing was built. Fix the failures above (install-prereqs.sh covers the packages).");
        return Ok(false);
    }

    let started = Instant::now();
    println!("\n── build {}", "─".repeat(54));
    println!(
        "{} with {} job(s), Cartographer at {}",
        context.platform.tag(),
        options.jobs,
        options.carto_cxx_flags_release()
    );

    clean::run(context)?;
    prepare_prefix(context)?;

    step(context, &ABSEIL, || {
        // C++17 here and in Cartographer, so absl::string_view is
        // std::string_view on both sides of the link.
        cmake_build(context, &ABSEIL, options, &["-DABSL_PROPAGATE_CXX_STD=ON", "-DBUILD_TESTING=OFF"])
    })?;
    step(context, &PROTOBUF, || {
        cmake_build(context, &PROTOBUF, options, &["-Dprotobuf_BUILD_TESTS=OFF", "-Dprotobuf_BUILD_SHARED_LIBS=OFF"])
    })?;
    step(context, &CERES, || {
        // Eigen's sparse solver is all the pose graph needs; SuiteSparse's
        // detection in 2.1 is unreliable anyway.
        cmake_build(
            context,
            &CERES,
            options,
            &[
                "-DBUILD_TESTING=OFF",
                "-DBUILD_EXAMPLES=OFF",
                "-DBUILD_BENCHMARKS=OFF",
                "-DSUITESPARSE=OFF",
                "-DCXSPARSE=OFF",
                "-DACCELERATESPARSE=OFF",
                "-DCUDA=OFF",
            ],
        )
    })?;
    step(context, &CARTOGRAPHER, || build_cartographer(context, options))?;

    let prefix = context.layout.prefix();
    context
        .runner
        .write(&prefix.join("VERSION"), &pins::version_file(&context.platform, options.carto_cxx_flags_release()))?;

    println!("\n── pack {}", "─".repeat(55));
    context.runner.create_dir(&context.out)?;
    let dependencies: Vec<&str> = pins::DEPENDENCIES.iter().map(|pin| pin.name).collect();
    pack::pack(context, &context.platform.dependencies_tarball(), &dependencies, &[])?;
    pack::pack(context, &context.platform.cartographer_tarball(), &[CARTOGRAPHER.name], &["VERSION"])?;

    if context.runner.dry_run {
        println!("\n--dry-run: nothing was built, so there is nothing to verify.");
        return Ok(true);
    }
    println!();
    let report: Report = verify::run(context);
    report.print("verify");
    println!("\nbuilt in {}", elapsed(started));
    Ok(!report.failed())
}

/// /opt is root's, so the prefix is made by sudo and handed to this user;
/// every install after that runs unprivileged.
fn prepare_prefix(context: &Context) -> Result<()> {
    let prefix = context.layout.prefix();
    let prefix = prefix.to_str().ok_or_else(|| anyhow!("prefix is not UTF-8"))?;
    let user = query("id", &["-un"]).ok_or_else(|| anyhow!("cannot read this user's name"))?;
    let group = query("id", &["-gn"]).ok_or_else(|| anyhow!("cannot read this user's group"))?;
    let owner = format!("{}:{}", user.trim(), group.trim());
    println!("\n══ {prefix}");
    context.runner.run("prefix", "sudo", &["mkdir", "-p", prefix])?;
    context.runner.run("prefix", "sudo", &["chown", &owner, prefix])?;
    if !context.runner.dry_run {
        println!("   ✓ created, owned by {owner}");
    }
    Ok(())
}

fn step(context: &Context, pin: &Pin, build: impl FnOnce() -> Result<()>) -> Result<()> {
    println!("\n══ {} {}", pin.name, pin.git_ref);
    let started = Instant::now();
    build()?;
    if !context.runner.dry_run {
        println!("   ✓ built and installed ({})", elapsed(started));
    }
    Ok(())
}

/// One commit, whether `git_ref` is a tag or a sha.
fn clone(context: &Context, pin: &Pin) -> Result<()> {
    let source = context.layout.source_dir(pin.name);
    let source = source.to_str().ok_or_else(|| anyhow!("work directory is not UTF-8"))?;
    let runner = &context.runner;
    runner.run(pin.name, "git", &["init", "-q", source])?;
    runner.run(pin.name, "git", &["-C", source, "fetch", "-q", "--depth", "1", pin.url, pin.git_ref])?;
    runner.run(pin.name, "git", &["-C", source, "checkout", "-q", "FETCH_HEAD"])?;
    Ok(())
}

/// The arguments every build shares. Static, position-independent (pycarto
/// is a shared object that links all of it), C++17, installed into the prefix
/// and looking there first.
fn common_arguments(context: &Context, pin: &Pin) -> Vec<String> {
    let prefix = context.layout.prefix().display().to_string();
    vec![
        "-S".into(),
        context.layout.source_dir(pin.name).display().to_string(),
        "-B".into(),
        context.layout.build_dir(pin.name).display().to_string(),
        "-G".into(),
        "Ninja".into(),
        "-DCMAKE_BUILD_TYPE=Release".into(),
        format!("-DCMAKE_INSTALL_PREFIX={prefix}"),
        format!("-DCMAKE_PREFIX_PATH={prefix}"),
        "-DCMAKE_POSITION_INDEPENDENT_CODE=ON".into(),
        "-DCMAKE_CXX_STANDARD=17".into(),
        "-DBUILD_SHARED_LIBS=OFF".into(),
    ]
}

/// Configure, build and install. The build's install_manifest.txt is what
/// the tarball packs.
fn cmake_build(context: &Context, pin: &Pin, options: &Options, extra: &[&str]) -> Result<()> {
    clone(context, pin)?;
    configure_build_install(context, pin, options, common_arguments(context, pin), extra, &[])
}

fn configure_build_install(
    context: &Context,
    pin: &Pin,
    options: &Options,
    mut configure: Vec<String>,
    extra: &[&str],
    targets: &[&str],
) -> Result<()> {
    configure.extend(extra.iter().map(|argument| argument.to_string()));
    let configure: Vec<&str> = configure.iter().map(String::as_str).collect();
    context.runner.run(pin.name, "cmake", &configure)?;

    let build_dir = context.layout.build_dir(pin.name).display().to_string();
    let jobs = format!("-j{}", options.jobs);
    let mut build = vec!["--build", build_dir.as_str(), jobs.as_str()];
    if !targets.is_empty() {
        build.push("--target");
        build.extend_from_slice(targets);
    }
    context.runner.run(pin.name, "cmake", &build)?;
    context.runner.run(pin.name, "cmake", &["--install", &build_dir])?;
    Ok(())
}

fn build_cartographer(context: &Context, options: &Options) -> Result<()> {
    clone(context, &CARTOGRAPHER)?;
    patch_cartographer(context)?;

    let prefix = context.layout.prefix().display().to_string();
    let flags = format!("-DCMAKE_CXX_FLAGS_RELEASE={}", options.carto_cxx_flags_release());
    // Protobuf is named file by file: FindProtobuf would otherwise take
    // whichever protoc is first on PATH, and /usr/local/bin/protoc is 27.2.
    let extra = [
        flags,
        format!("-DCeres_DIR={prefix}/lib/cmake/Ceres"),
        format!("-Dabsl_DIR={prefix}/lib/cmake/absl"),
        format!("-DProtobuf_INCLUDE_DIR={prefix}/include"),
        format!("-DProtobuf_LIBRARY={prefix}/lib/libprotobuf.a"),
        format!("-DProtobuf_LITE_LIBRARY={prefix}/lib/libprotobuf-lite.a"),
        format!("-DProtobuf_PROTOC_LIBRARY={prefix}/lib/libprotoc.a"),
        format!("-DProtobuf_PROTOC_EXECUTABLE={prefix}/bin/protoc"),
        // Ubuntu 22.04's Boost 1.74 ships a BoostConfig.cmake without the
        // iostreams component config; the FindBoost module works.
        "-DBoost_NO_BOOST_CMAKE=ON".to_string(),
        "-DBUILD_GRPC=OFF".to_string(),
    ];
    let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
    configure_build_install(
        context,
        &CARTOGRAPHER,
        options,
        common_arguments(context, &CARTOGRAPHER),
        &extra,
        &CARTOGRAPHER_TARGETS,
    )
}

/// The mapping server's two patches (mapping_server/cartographer/build.sh),
/// and one for the installed CMake package.
const WERROR_UNINITIALIZED: &str = r#"google_add_flag(GOOG_CXX_FLAGS "-Werror=uninitialized")"#;
const WNO_ERROR: &str = r#"
    google_add_flag(GOOG_CXX_FLAGS "-Wno-error=maybe-uninitialized")
    google_add_flag(GOOG_CXX_FLAGS "-Wno-error=uninitialized")"#;
const CERES_WITH_SUITESPARSE: &str = "find_package(Ceres REQUIRED COMPONENTS SuiteSparse)";
const CERES_PLAIN: &str = "find_package(Ceres REQUIRED)";
const CERES_INCLUDE: &str = "target_include_directories(${PROJECT_NAME} SYSTEM PUBLIC\n  \"${CERES_INCLUDE_DIRS}\")\n";

/// Newer GCC flags Eigen with -Wmaybe-uninitialized false positives that
/// -Werror=uninitialized makes fatal; and Ceres above is built without
/// SuiteSparse, which Cartographer only asks for as a performance component.
///
/// Ceres 2.x no longer sets CERES_INCLUDE_DIRS, so Cartographer's include of it
/// is an empty path, which CMake reads as Cartographer's source directory and
/// exports into CartographerTargets.cmake. Anything linking the installed
/// package then fails to configure, the build machine's path not existing on
/// it. Ceres::ceres, linked on the next line, carries Ceres's includes anyway.
///
/// Each patch must apply. A new pin whose text differs would otherwise build
/// unpatched and fail an hour later, somewhere else.
fn patch_cartographer(context: &Context) -> Result<()> {
    let source = context.layout.source_dir(CARTOGRAPHER.name);
    let functions = source.join("cmake/functions.cmake");
    let lists = source.join("CMakeLists.txt");
    if context.runner.dry_run {
        println!("   patch {} (-Wno-error=uninitialized)", functions.display());
        println!("   patch {} (Ceres without SuiteSparse)", lists.display());
        println!("   patch {} (no CERES_INCLUDE_DIRS)", lists.display());
        return Ok(());
    }
    for (path, patch) in [
        (&functions, add_wno_error as fn(&str) -> Result<String>),
        (&lists, drop_suitesparse),
        (&lists, drop_ceres_include),
    ] {
        let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let patched = patch(&text).with_context(|| format!("patching {}", path.display()))?;
        fs::write(path, patched).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

fn add_wno_error(text: &str) -> Result<String> {
    if !text.contains(WERROR_UNINITIALIZED) {
        bail!("no `{WERROR_UNINITIALIZED}` to follow with -Wno-error");
    }
    Ok(text.replacen(WERROR_UNINITIALIZED, &format!("{WERROR_UNINITIALIZED}{WNO_ERROR}"), 1))
}

fn drop_suitesparse(text: &str) -> Result<String> {
    if !text.contains(CERES_WITH_SUITESPARSE) {
        bail!("no `{CERES_WITH_SUITESPARSE}` to replace");
    }
    Ok(text.replacen(CERES_WITH_SUITESPARSE, CERES_PLAIN, 1))
}

fn drop_ceres_include(text: &str) -> Result<String> {
    if !text.contains(CERES_INCLUDE) {
        bail!("no include of CERES_INCLUDE_DIRS to remove");
    }
    Ok(text.replacen(CERES_INCLUDE, "", 1))
}

pub fn elapsed(started: Instant) -> String {
    let seconds = started.elapsed().as_secs();
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_default_to_three_and_two_with_low_mem() {
        assert_eq!(Options::resolve(None, false).jobs, 3);
        assert_eq!(Options::resolve(None, true).jobs, 2);
        assert_eq!(Options::resolve(Some(6), true).jobs, 6);
        assert_eq!(Options::resolve(None, false).carto_cxx_flags_release(), "-O3 -DNDEBUG");
        assert!(Options::resolve(None, true).carto_cxx_flags_release().starts_with("-O1 "));
    }

    #[test]
    fn wno_error_follows_werror() {
        let text = "  google_add_flag(GOOG_CXX_FLAGS \"-Werror=uninitialized\")\n  next\n";
        let patched = add_wno_error(text).unwrap();
        assert!(patched.contains("-Werror=uninitialized\")\n    google_add_flag(GOOG_CXX_FLAGS \"-Wno-error=maybe-uninitialized\")"));
        assert!(patched.ends_with("\"-Wno-error=uninitialized\")\n  next\n"));
    }

    #[test]
    fn suitesparse_is_dropped_and_a_missing_line_is_an_error() {
        let patched = drop_suitesparse("find_package(Ceres REQUIRED COMPONENTS SuiteSparse)\n").unwrap();
        assert_eq!(patched, "find_package(Ceres REQUIRED)\n");
        assert!(drop_suitesparse("find_package(Ceres REQUIRED)\n").is_err());
        assert!(add_wno_error("nothing here").is_err());
    }

    #[test]
    fn ceres_include_is_dropped_and_a_missing_one_is_an_error() {
        let text = "a\ntarget_include_directories(${PROJECT_NAME} SYSTEM PUBLIC\n  \"${CERES_INCLUDE_DIRS}\")\ntarget_link_libraries(${PROJECT_NAME} PUBLIC ${CERES_LIBRARIES})\n";
        let patched = drop_ceres_include(text).unwrap();
        assert_eq!(patched, "a\ntarget_link_libraries(${PROJECT_NAME} PUBLIC ${CERES_LIBRARIES})\n");
        assert!(drop_ceres_include(&patched).is_err());
    }
}
