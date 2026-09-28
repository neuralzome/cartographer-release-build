#!/bin/sh
# Build crb and put it on PATH. Run it again after every pull that touches
# src/ — `crb doctor` warns when the installed copy is older than
# the sources, but only once you are already running it.
set -eu

cd "$(dirname "$0")"

# rustup installs cargo under ~/.cargo/bin, which a shell only has on PATH once
# its rc file sources ~/.cargo/env — not in the session rustup ran in, and not
# at all when it was installed with --no-modify-path.
if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
    . "$HOME/.cargo/env"
fi
command -v cargo >/dev/null 2>&1 || {
    echo "cargo is not installed; run ./install-prereqs.sh first" >&2
    exit 1
}

cargo build --release

# /usr/local/bin rather than ~/.cargo/bin, as msd: the next person on the
# machine, or the container's other shell, has to find it too.
DESTINATION=${DESTINATION:-/usr/local/bin}

if [ -w "$DESTINATION" ]; then
    install -m 0755 target/release/crb "$DESTINATION/crb"
else
    echo "installing to $DESTINATION needs root:"
    sudo install -m 0755 target/release/crb "$DESTINATION/crb"
fi

# By full path, not through PATH: `command -v` returning nothing would abort
# the script under `set -e` AFTER the install had already succeeded.
echo
"$DESTINATION/crb" --version

RESOLVED=$(command -v crb 2>/dev/null || true)
if [ "$RESOLVED" != "$DESTINATION/crb" ]; then
    echo
    echo "note: \`crb\` on your PATH is ${RESOLVED:-nothing}, not $DESTINATION/crb"
fi
