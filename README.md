# `crb` — cartographer release build

A small tool that builds [Cartographer](https://github.com/cartographer-project/cartographer)
and the libraries it needs from source, and packs them as two tarballs for the
machine it runs on. Build once per platform, publish the tarballs, and other
machines extract them instead of compiling Cartographer themselves.

```bash
./install-prereqs.sh             # once: toolchain, CMake, Ninja, the apt -dev packages, rust (--check)
./install.sh                     # build crb and put it on PATH

crb doctor                       # check the preconditions, read-only
crb build                        # clean, build everything, pack, verify
crb verify                       # check the installation and the tarballs, read-only
crb clean                        # remove /opt/carto, build/ and this platform's tarballs

  --jobs N                       # parallel compiles (build, doctor): 3, or 2 with --low-mem
  --low-mem                      # compile Cartographer at -O1 (build, doctor)
  --out <dir>                    # where the tarballs go (default: dist/)
  --dry-run                      # print the commands, change nothing
  --verbose                      # show every command's output instead of logging it
  --repo <path>                  # checkout to work from (default: search upward)
```

## What it makes

For `<platform>` = `<distro><version>-<arch>`, e.g. `ubuntu22.04-aarch64`:

| File | Holds |
| --- | --- |
| `dependencies-<platform>.tar.gz` | abseil `20220623.1`, protobuf `v21.12`, Ceres `2.1.0` |
| `cartographer-<platform>.tar.gz` | Cartographer 2.0.0 at `877157a`, its Lua `configuration_files/`, `VERSION` |
| `*.tar.gz.sha256` | checksums in `sha256sum -c` format |

Install on another machine of the same platform with:

```bash
sudo mkdir -p /opt/carto
sudo tar -xzf dependencies-<platform>.tar.gz -C /opt/carto
sudo tar -xzf cartographer-<platform>.tar.gz -C /opt/carto
crb verify
```

- **Everything is static and position-independent,** so it links into
  executables and shared libraries alike.
- **Each tarball holds only what its own builds installed.** The list is taken
  from CMake's `install_manifest.txt`.
- **`VERSION`** records the platform, every pinned version, and the flags
  Cartographer was compiled with.

## Versions

| | Version |
| --- | --- |
| Cartographer | 2.0.0 + 12 commits: `877157a0d91788a7700221d87232d412cb3c1ef4`, upstream master as of 2024-01-05 |
| Ceres | 2.1.0 |
| protobuf | 3.21.12 (`v21.12`) |
| abseil | 20220623.1 |

- **Ceres stays at 2.1** because Cartographer still uses the
  `LocalParameterization` API that Ceres 2.2 removed.
- **protobuf and abseil are built from source**, not taken from apt. This keeps
  them at known versions. It also keeps them separate from any other protobuf on
  the machine: Cartographer is pointed at the protobuf under `/opt/carto` file
  by file. Otherwise CMake could pick up a different `protoc` from PATH, and the
  generated headers would not match the library.

The versions are pinned in `src/pins.rs`. Changing one means rebuilding for
every platform.

## Platforms

- **Architectures:** x86_64 and aarch64.
- **Distros:** static libraries built against one distro's glog, gflags and
  glibc only work on that distro. So the tarballs carry the platform in their
  name. The intended target is **Ubuntu 22.04**; `doctor` warns on anything
  else, but still builds.

## What `build` does

1. **Checks the preconditions** (see `doctor` below). Any failure stops it
   before anything is touched.
2. **Cleans.** Every build starts from nothing, even when `/opt/carto` is
   already complete.
3. **Creates `/opt/carto`** with sudo and gives it to you. Every install after
   that runs unprivileged.
4. **Builds abseil, protobuf, Ceres and Cartographer, in that order.**
   - Each is fetched as a single commit at its pin.
   - Each is built as Release (`-O3 -DNDEBUG`), static, `-fPIC`, C++17.
5. **Patches Cartographer twice:**
   - adds `-Wno-error=maybe-uninitialized`, for GCC's false positives in Eigen;
   - builds against Ceres without SuiteSparse.

   **If either patch doesn't apply, the build stops.** Only the library and the
   four tools its install step needs are built; the test targets are skipped.
6. **Writes `VERSION`, both tarballs and their `.sha256`s.**
7. **Runs `verify`.**

**Command output goes to a log per step,** in `build/logs/`. When a step fails,
its last 40 lines are printed along with the log's path. `--verbose` streams
everything instead.

**A failed build leaves `build/` in place** for you to look at. The next `build`
or `clean` removes it.

**`clean` only removes this platform's tarballs,** not the whole `--out`
directory, so tarballs for other platforms can collect in `dist/`.

## Memory

A Cartographer source file can take 3–5 GB to compile at `-O3`. `doctor`
allows 4.5 GB per parallel job.

- **The default is 3 jobs,** which fits a 16 GB machine. Run too many and the
  compiler is killed partway through.
- **`--low-mem`** is for machines that still run out of memory. It applies to
  Cartographer only:
  - compiles at `-O1 -DNDEBUG --param ggc-min-expand=10 --param ggc-min-heapsize=32768`;
  - defaults to 2 jobs;
  - records the flags in `VERSION`, and `verify` warns about them.

  The result runs slower, so try `--jobs 2` without it first.

## `doctor`

`build` runs the same checks and refuses to start on a failure.

- **The platform:** Ubuntu 22.04, or a warning.
- **Tools on PATH:** git, cmake, ninja, g++, pkg-config, tar, sha256sum, sudo.
- **The apt packages Ceres and Cartographer compile against,** kept in step
  with `install-prereqs.sh`. They include gmock and gtest, because
  Cartographer's configure step needs them even though no test target is built.
- **Enough memory for `--jobs`.**
- **15 GB free disk** for the sources and build trees.
- **Whether sudo will prompt.** sudo is only used to create and remove
  `/opt/carto`.
- **Whether `crb` itself is current:** built from `src/` as it stands, and the
  copy in `/usr/local/bin` is that build.

## `verify`

Checks the installation in `/opt/carto`, whether it came from `build` or from
extracting the tarballs.

- **`VERSION`** names this platform and every pinned version, and records `-O3`
  (anything else is a warning).
- **Each library and its CMake config file are present.**
- **`/opt/carto/bin/protoc --version` reports 3.21.12.**
- **Cartographer's Lua configuration files are installed.**
- **`cartographer_print_configuration` runs and loads `trajectory_builder.lua`.**
- **This platform's tarballs match their `.sha256`s and hold what they
  should.** If there are no tarballs, this is skipped rather than failed.

## Tests

```bash
cargo test
```

Covers:

- reading the platform from `/etc/os-release`;
- writing and reading back `VERSION`;
- turning install manifests into tarball paths;
- both Cartographer patches;
- the jobs and low-memory defaults;
- the memory arithmetic;
- how commands are quoted for display.
