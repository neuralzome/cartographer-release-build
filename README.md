# `crb` — cartographer release build

Builds Cartographer and everything it needs that apt cannot supply, from source,
and packs them as two release tarballs for this platform. Run it by hand, once
per platform, and upload what it makes to a release of
[neuralzome/cartographer-build](https://github.com/neuralzome/cartographer-build).
ubr's `install.sh` fetches the tarballs into `/opt/carto`, so no machine builds
Cartographer again. Then ubr's `build.sh` compiles pycarto against them.

It lives in its own repository and, as `cartographer/`, in the ubr monorepo. It
finds itself by its `Cargo.toml` either way, and every path below is relative to
the checkout.

```bash
./install-prereqs.sh             # once: toolchain, CMake, Ninja, the apt -dev packages, rust (--check)
./install.sh                     # build and put crb on PATH

crb doctor                       # every precondition, read-only, safe any time
crb build                        # clean, build all four, pack, verify
crb verify                       # the installation and the tarballs, read-only
crb clean                        # /opt/carto, build/, this platform's tarballs

  --jobs N                       # parallel compiles (build, doctor): 3, or 2 with --low-mem
  --low-mem                      # Cartographer at -O1 (build, doctor)
  --out <dir>                    # where the tarballs go (default: dist/)
  --dry-run                      # print the commands, change nothing
  --verbose                      # every command's output, instead of a log per step
  --repo <path>                  # checkout to work from (default: search upward)
```

It is shaped like `mapping_server/devops/msd`, so what you know about one
carries over:

- the same `install-prereqs.sh` and `install.sh` pair, installing to
  `/usr/local/bin`;
- `doctor` and `verify` printing `✓ ! ✗ -`;
- `--dry-run`;
- the same staleness check on its own binary.

## What it makes

For `<platform>` = `<distro><version>-<arch>`, e.g. `ubuntu22.04-aarch64`:

| File | Holds |
| --- | --- |
| `dependencies-<platform>.tar.gz` | abseil `20220623.1`, protobuf `v21.12`, Ceres `2.1.0` |
| `cartographer-<platform>.tar.gz` | Cartographer `877157a`, its Lua `configuration_files/`, `VERSION` |
| `*.tar.gz.sha256` | `sha256sum` format, no path, so `sha256sum -c` works wherever the pair lands |

**Paths are relative to `/opt/carto`.** Both extract with
`tar -xzf <file> -C /opt/carto`.

**Each tarball holds only what its own builds installed.** The list comes from
their CMake `install_manifest.txt`, not from whatever happens to be in the
prefix.

**`VERSION`** records the platform, all four pins and the flags Cartographer was
compiled with. It is what `install.sh` compares with its own pins to decide
whether to fetch.

**Everything is static and position-independent.** pycarto is one shared object
that links all four libraries. At runtime it needs nothing from `/opt/carto`
except the Lua files, which a MapBuilder reads when it is made.

## Which platform

ubr runs **Ubuntu 22.04** everywhere: the laptop's container and the Orin's
JetPack 6. A static library built against one distro's glog, gflags and glibc
is only good on that distro. So the tarballs carry the platform in their name,
and `doctor` warns on anything that is not 22.04.

This workstation is 24.04. Build inside the ubr container on x86, and on an
Orin for aarch64.

## Why these versions

- **Cartographer and Ceres use the mapping server's pins**
  (`mapping_server/devops/docker/Dockerfile`), so the Cartographer that writes a
  map's `.pbstream` is the one that reads it.
- **Ceres stays at 2.1** because Cartographer still uses the
  `LocalParameterization` API that 2.2 removed.
- **abseil and protobuf are the versions the server links**, from Debian
  bookworm's apt. They are built from source here and never taken from the
  machine:
  - Ubuntu 22.04's apt copies are older.
  - `install.sh` puts **protobuf 27.2** in `/usr/local` for ubr's C++ nodes.

  So Cartographer is configured with protobuf named file by file under
  `/opt/carto`. Otherwise CMake's FindProtobuf takes the first `protoc` on PATH,
  which is 27.2, and the generated headers no longer match the library.

The pins live in `src/pins.rs`. Changing one means a new release for every
platform.

## What build does

1. **The preconditions `doctor` checks.** Any failure stops it before anything
   is touched.
2. **`clean`.** Every build starts from nothing, even when `/opt/carto` is
   already complete. A build on top of an old prefix can link against what it
   should have replaced.
3. **`/opt/carto`, made with sudo and given to you.** Every install after that
   runs unprivileged.
4. **abseil, protobuf, Ceres, Cartographer, in that order.** Each is fetched as
   one commit at its pin. Each is built as Release: `-O3 -DNDEBUG`, static,
   `-fPIC`, C++17. C++17 on both sides matters: it makes `absl::string_view`
   `std::string_view` in abseil and in Cartographer alike.
5. **Cartographer gets the mapping server's two patches:**
   - `-Wno-error=maybe-uninitialized`, for GCC's false positives in Eigen;
   - Ceres without SuiteSparse.

   **Each patch must apply, or the build stops.** A new pin whose CMake text
   differs would otherwise build unpatched and fail an hour later somewhere
   unrelated. Only the library and the four tools its install rules need are
   built. The ~130 test targets are skipped, and several of them do not compile
   against current gmock.
6. **`VERSION`, the two tarballs and their `.sha256`s.**
7. **`verify`.**

**Command output goes to a log per step,** in `build/logs/`. When a
step fails, its last 40 lines are printed with the error and the log's path.
`--verbose` streams everything instead.

**A failed build leaves `build/` in place** to look at. The next
`build` or `clean` removes it.

**`clean` removes this platform's tarballs, not the whole `--out` directory.**
The other platforms' tarballs sit beside them in `dist/` until they are
uploaded. And a mistyped `--out` must never cost more than two files.

## Memory

A Cartographer translation unit takes 3-5 GB to compile at `-O3`. `doctor`
counts 4.5 GB a job against `/proc/meminfo`, the same figure msd uses.

- **The default is 3 jobs, which fits a 16 GB Orin NX.** Too many jobs and the
  compiler is killed partway through, with nothing but `Killed` in the log.
- **`--low-mem`** is py-cartographer's `LOW_MEM=1`, for Cartographer only:
  - it compiles at `-O1 -DNDEBUG --param ggc-min-expand=10 --param ggc-min-heapsize=32768`,
    so GCC collects its garbage early and often;
  - it defaults to 2 jobs;
  - `VERSION` records the flags, and `verify` warns about them.

  The result localises slower, so try `--jobs 2` at `-O3` first.

## Preconditions

`doctor` runs them; `build` runs the same set and refuses on a failure.

- **The platform:** Ubuntu 22.04, or a warning that the tarballs will be no use
  to ubr.
- **The tools on PATH:** git, cmake, ninja, g++, pkg-config, tar, sha256sum and
  sudo.
- **The apt packages Ceres and Cartographer compile against**, by `dpkg-query`.
  The list is kept in step with `install-prereqs.sh`. It includes gmock and
  gtest: Cartographer's configure step requires them even though no test
  target is built.
- **Memory for `--jobs`.** See above.
- **15 GB free** for the four source trees and their build trees.
- **Whether sudo will prompt.** It is used only to create and remove
  `/opt/carto`. The build waits at the prompt, so stay for its first minute.
- **crb itself is current:** whether the binary you are running was built from
  `src/` as it stands, and whether `/usr/local/bin/crb` is that
  binary. A stale crb builds with older pins than the tree says, and says
  nothing.

## What verify proves

`verify` reads the prefix, however it was filled: by `build`, or by
`install.sh` extracting the release. So it answers the same question on the
build machine and on a robot: is this Cartographer the one the pins say, and
does it run.

- **`VERSION`** names this platform, every pin, and `-O3` (a warning otherwise).
- **Each library and its CMake config file are present.** These are what
  pycarto's CMake finds.
- **`/opt/carto/bin/protoc --version` reports 3.21.12.** That is the protoc
  Cartographer's `.pb.h` files came from, not the 27.2 one on PATH.
- **The Lua configuration files are installed.** A prefix without them builds
  pycarto fine and fails at the first map.
- **`cartographer_print_configuration` runs and loads `trajectory_builder.lua`.**
  It is linked against everything above and resolves the same Lua includes a
  MapBuilder's configuration will. It running is the proof the pieces fit.
- **This platform's tarballs match their `.sha256`s and hold what each should.**
  Skipped, not failed, when there are none: a machine that installed the
  release has a prefix and no tarballs.

## pycarto

In the ubr monorepo, `bindings.cpp` and `CMakeLists.txt` sit beside crb as
symlinks to `mapping_server/cartographer/`, so the robot and the mapping server
compile the same binding from the same file. They are not in this repository. `build.sh` compiles it against `/opt/carto`.
crb does not, because pycarto belongs to the Python it is built for, and that is
the venv on each machine, not the release.

## Tests

`cargo test` runs twelve, covering:

- the platform read from `/etc/os-release`;
- `VERSION` written and read back, and the protoc version derived from the pin;
- manifest paths made relative, refusing any outside the prefix;
- both Cartographer patches, including refusing to build when a patch does not
  apply;
- the jobs defaults and the low-memory flags;
- the memory arithmetic;
- how commands are quoted for display.

Everything that compiles is verified by running `crb build`.
