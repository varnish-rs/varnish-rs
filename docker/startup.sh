#!/bin/sh

if [ ! -d /app/.github ] || [ ! -d ~/.cache ]; then
    echo " "
    echo "ERROR: Docker container was not started properly."
    echo "       Use   just docker-run-76  or another version."
    exit 1
fi

export PATH="$PATH:~/.local/bin/"

export CARGO_TARGET_DIR=~/.cache/target
if [ ! -f "$CARGO_HOME/env" ]; then
    echo "Downloading and installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi
. "$CARGO_HOME/env"

if ! command -v cargo-insta; then
  echo "cargo-insta is not found. Installing..." >&2
  curl -LsSf https://insta.rs/install.sh | sh
fi

if ! command -v cargo-binstall; then
  echo "cargo-binstall is not found. Installing..." >&2
  curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
fi

if ! grep -q "alias j=" ~/.bashrc; then
  echo "alias j='just'" >> ~/.bashrc
fi

echo "##################################################"
echo "##  Welcome to the Varnish development container"
echo "##  Use 'just' to see the available commands."
echo "##################################################"
exec "$@"
