#!/usr/bin/env bash
# Install what a machine needs before ./install.sh and `crb build`,
# on Ubuntu/Debian. Unattended and idempotent: re-running skips what is present.
#
#   ./install-prereqs.sh               # everything
#   ./install-prereqs.sh --check       # report only, install nothing
#   ./install-prereqs.sh --skip-rust   # opt out of a step
#   ./install-prereqs.sh --only build
#
# The build packages are the ones Ceres and Cartographer compile against from
# apt. abseil and protobuf are not among them: crb builds both from source, at
# the mapping server's versions. `crb doctor` checks the same list.

set -euo pipefail

# The oldest rustc Cargo.lock builds with (clap 4.6).
RUST_MIN=1.85

# Kept in step with APT_PACKAGES in src/checks.rs. gmock and gtest only because
# Cartographer's configure step requires them; no test target is built.
BUILD_PACKAGES=(
  libboost-iostreams-dev libcairo2-dev libeigen3-dev libgflags-dev
  libgoogle-glog-dev liblua5.3-dev liblapack-dev libblas-dev zlib1g-dev
  libgmock-dev libgtest-dev
)

STEPS=(base build rust)
SKIP=()
ONLY=()
CHECK_ONLY=false
NEEDS_RELOGIN=false

# ── output ────────────────────────────────────────────────────────────────────

if [ -t 1 ]; then
  BOLD=$'\033[1m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; RED=$'\033[31m'; DIM=$'\033[2m'; OFF=$'\033[0m'
else
  BOLD=''; GREEN=''; YELLOW=''; RED=''; DIM=''; OFF=''
fi

step()  { printf '\n%s══ %s%s\n' "$BOLD" "$1" "$OFF"; }
ok()    { printf '   %s✓%s %s\n' "$GREEN" "$OFF" "$1"; }
info()  { printf '   %s·%s %s\n' "$DIM" "$OFF" "$1"; }
warn()  { printf '   %s!%s %s\n' "$YELLOW" "$OFF" "$1"; }
fail()  { printf '   %s✗%s %s\n' "$RED" "$OFF" "$1"; }
die()   { fail "$1"; exit 1; }

usage() {
  sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  printf '\nSteps: %s\n' "${STEPS[*]}"
}

# ── arguments ─────────────────────────────────────────────────────────────────

while [ $# -gt 0 ]; do
  case "$1" in
    --check)   CHECK_ONLY=true; shift ;;
    --only)    IFS=',' read -r -a ONLY <<< "${2:?--only needs a comma-separated step list}"; shift 2 ;;
    --skip-*)  SKIP+=("${1#--skip-}"); shift ;;
    -h|--help) usage; exit 0 ;;
    *)         usage; die "unknown argument: $1" ;;
  esac
done

for name in "${ONLY[@]}" "${SKIP[@]}"; do
  # shellcheck disable=SC2076
  [[ " ${STEPS[*]} " =~ " ${name} " ]] || die "unknown step '${name}' (valid: ${STEPS[*]})"
done

wanted() {
  local name=$1
  # shellcheck disable=SC2076
  [[ " ${SKIP[*]} " =~ " ${name} " ]] && return 1
  if [ ${#ONLY[@]} -gt 0 ]; then
    # shellcheck disable=SC2076
    [[ " ${ONLY[*]} " =~ " ${name} " ]]
    return
  fi
  return 0
}

# `version_ge 1.90.0 1.85` is true.
version_ge() {
  [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -1)" = "$2" ]
}

# ── platform ──────────────────────────────────────────────────────────────────

[ "$(uname -s)" = "Linux" ] || die "this script targets Ubuntu/Debian"
command -v apt-get >/dev/null 2>&1 || die "no apt-get — this script targets Ubuntu/Debian"

# shellcheck disable=SC1091
. /etc/os-release
DISTRO="${ID:-unknown}${VERSION_ID:-}"

if [ "$(id -u)" -eq 0 ]; then
  SUDO=""
  TARGET_USER=${SUDO_USER:-root}
else
  command -v sudo >/dev/null 2>&1 || die "not root and sudo is missing — install sudo or run as root"
  SUDO="sudo"
  TARGET_USER=$(id -un)
fi
TARGET_HOME=$(getent passwd "$TARGET_USER" | cut -d: -f6)

APT_UPDATED=false
apt_update_once() {
  $APT_UPDATED && return 0
  $SUDO apt-get update -qq
  APT_UPDATED=true
}
apt_install() {
  apt_update_once
  $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "$@" >/dev/null
}

# rustup writes into $HOME, so under `sudo ./install-prereqs.sh` it would
# otherwise land in /root and be invisible to whoever runs crb.
as_user() {
  if [ "$TARGET_USER" = "$(id -un)" ]; then
    "$@"
  else
    sudo -u "$TARGET_USER" -H "$@"
  fi
}

# cargo lives under $HOME, so it is not on PATH in this process even right
# after installing.
user_sh() {
  as_user bash -c '[ -s "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
'"$1"
}

# Install whichever of "$@" dpkg does not have.
apt_packages() {
  local missing=() pkg
  for pkg in "$@"; do
    dpkg -s "$pkg" >/dev/null 2>&1 || missing+=("$pkg")
  done
  if [ ${#missing[@]} -eq 0 ]; then
    ok "all present"
    return
  fi
  info "missing: ${missing[*]}"
  if $CHECK_ONLY; then return 0; fi
  apt_install "${missing[@]}"
  ok "installed ${missing[*]}"
}

printf '%scartographer release build prerequisites%s  —  %s %s, user %s\n' \
  "$BOLD" "$OFF" "$DISTRO" "$(uname -m)" "$TARGET_USER"
if $CHECK_ONLY; then info "--check: reporting only, nothing will be installed"; fi
if [ "$DISTRO" != "ubuntu22.04" ]; then
  warn "ubr runs Ubuntu 22.04; tarballs built on $DISTRO are named for it and are no use to ubr"
fi

# ── base ──────────────────────────────────────────────────────────────────────

install_base() {
  # build-essential: g++ for the builds, and cc for cargo's linker. git: every
  # source is fetched at its pin. ninja: every CMake build uses it.
  step "base packages (a C++ toolchain, CMake, Ninja, git)"
  apt_packages build-essential cmake ninja-build git ca-certificates curl pkg-config
}

# ── build ─────────────────────────────────────────────────────────────────────

install_build() {
  step "what Ceres and Cartographer compile against"
  apt_packages "${BUILD_PACKAGES[@]}"
}

# ── rust ──────────────────────────────────────────────────────────────────────

install_rust() {
  step "rust ${RUST_MIN}+ (crb itself)"
  local version
  version=$(user_sh 'rustc --version' 2>/dev/null | awk '{print $2}' || true)
  if [ -n "$version" ] && version_ge "$version" "$RUST_MIN"; then
    ok "rustc $version"
    return
  fi

  if [ -n "$version" ]; then
    info "rustc $version is older than $RUST_MIN; crb's Cargo.lock will not build with it"
  else
    info "not installed"
  fi
  if $CHECK_ONLY; then return 0; fi

  if user_sh 'command -v rustup' >/dev/null 2>&1; then
    user_sh 'rustup toolchain install stable --profile minimal && rustup default stable' >/dev/null
  else
    [ -n "$version" ] && warn "rustc $version is not from rustup; installing rustup's, which takes precedence on PATH"
    as_user bash -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal"
    local rc
    for rc in "$TARGET_HOME/.bashrc" "$TARGET_HOME/.profile"; do
      [ -f "$rc" ] || continue
      grep -qF '.cargo/env' "$rc" && continue
      # shellcheck disable=SC2016
      printf '\n%s\n' 'source "$HOME/.cargo/env"' | as_user tee -a "$rc" >/dev/null
      info "added cargo to $(basename "$rc")"
    done
    NEEDS_RELOGIN=true
  fi
  version=$(user_sh 'rustc --version' | awk '{print $2}')
  version_ge "$version" "$RUST_MIN" || die "rustc is $version after installing; expected $RUST_MIN or newer"
  ok "rustc $version"
}

for name in "${STEPS[@]}"; do
  wanted "$name" && "install_$name"
done

if $CHECK_ONLY; then
  printf '\n%s--check only — nothing was installed.%s\n' "$DIM" "$OFF"
  exit 0
fi

if $NEEDS_RELOGIN; then
  printf '\n%sLog out and back in%s (or `exec $SHELL -l`) so cargo is on PATH.\n' "$BOLD" "$OFF"
fi

cat <<EOF

${BOLD}Next steps${OFF}

  ./install.sh        # build crb, install to /usr/local/bin
  crb doctor
  crb build                      # about an hour; then upload dist/*
EOF
