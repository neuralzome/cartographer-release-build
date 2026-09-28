//! crb — cartographer release build. See README.md for what each command and
//! check means and why it exists.

mod build;
mod checks;
mod clean;
mod host;
mod layout;
mod pack;
mod pins;
mod platform;
mod report;
mod runner;
mod verify;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use build::Options;
use layout::Layout;
use platform::Platform;
use runner::Runner;

#[derive(Parser)]
#[command(
    name = "crb",
    version,
    about = "Build Cartographer and its dependencies from source, packed as release tarballs",
    long_about = "Builds abseil, protobuf, Ceres and Cartographer from source at the mapping \
                  server's pins into /opt/carto, packs them as dependencies-<platform>.tar.gz and \
                  cartographer-<platform>.tar.gz, and verifies both. Run once per platform; \
                  upload the tarballs to a GitHub release for install.sh to fetch."
)]
struct Cli {
    /// Checkout to work from. Defaults to searching upward from the cwd.
    #[arg(long, global = true)]
    repo: Option<PathBuf>,

    /// Where the tarballs go. Defaults to dist/ in the checkout.
    #[arg(long, global = true)]
    out: Option<PathBuf>,

    /// Print the commands instead of running them.
    #[arg(long, global = true)]
    dry_run: bool,

    /// Show every command's output instead of logging it to build/logs.
    #[arg(long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Args, Clone, Copy)]
struct BuildArgs {
    /// Parallel compiles. Default 3 (fits 16 GB at -O3), or 2 with --low-mem.
    #[arg(long)]
    jobs: Option<u32>,
    /// Compile Cartographer at -O1 with GCC collecting garbage aggressively,
    /// for machines that run out of memory. Recorded in VERSION; it localises
    /// slower.
    #[arg(long)]
    low_mem: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Every precondition. Read-only, safe at any time.
    Doctor {
        #[command(flatten)]
        options: BuildArgs,
    },
    /// Clean, build all four from source, pack both tarballs, verify.
    Build {
        #[command(flatten)]
        options: BuildArgs,
    },
    /// Check /opt/carto and this platform's tarballs. Read-only.
    Verify,
    /// Remove /opt/carto, build/ and this platform's tarballs.
    Clean,
}

/// What every command works with.
pub struct Context {
    pub layout: Layout,
    pub platform: Platform,
    pub out: PathBuf,
    pub runner: Runner,
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("\ncrb: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// `Ok(false)` is a clean run that found problems; `Err` is crb itself being
/// unable to proceed.
fn run() -> Result<bool> {
    let cli = Cli::parse();
    let start = cli.repo.clone().unwrap_or(std::env::current_dir()?);
    let layout = Layout::discover(&start)?;
    let platform = Platform::detect()?;
    let out = cli.out.clone().unwrap_or_else(|| layout.default_out());
    let runner = Runner { dry_run: cli.dry_run, verbose: cli.verbose, log_dir: layout.log_dir() };

    println!("crb — cartographer release build ({}), {}", platform.tag(), layout.root.display());
    let context = Context { layout, platform, out, runner };

    match cli.command {
        Commands::Doctor { options } => {
            let report = checks::run(&context, &Options::resolve(options.jobs, options.low_mem));
            println!();
            report.print("preconditions");
            Ok(!report.failed())
        }
        Commands::Build { options } => {
            println!();
            build::run(&context, &Options::resolve(options.jobs, options.low_mem))
        }
        Commands::Verify => {
            let report = verify::run(&context);
            println!();
            report.print("verify");
            Ok(!report.failed())
        }
        Commands::Clean => {
            clean::run(&context)?;
            Ok(true)
        }
    }
}
