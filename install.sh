#!/bin/sh
# Installs the latest splice release for this machine. It is shell because it runs
# before any splice binary exists; everything else in the repository is Rust.
#
#   curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/Danglebary/splice/main/install.sh | sh
#
# SPLICE_INSTALL_DIR names the directory the binary lands in, ~/.local/bin by default.
set -eu

repository="Danglebary/splice"
install_dir="${SPLICE_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)" in
    Linux) system="unknown-linux-musl" ;;
    Darwin) system="apple-darwin" ;;
    *) echo "splice install: no release is built for $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
    x86_64 | amd64) machine="x86_64" ;;
    arm64 | aarch64) machine="aarch64" ;;
    *) echo "splice install: no release is built for $(uname -m)" >&2; exit 1 ;;
esac

target="$machine-$system"
archive="splice-$target.tar.gz"
url="https://github.com/$repository/releases/latest/download/$archive"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location --max-time 120 \
    --output "$work/$archive" "$url"
curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location --max-time 30 \
    --output "$work/$archive.sha256" "$url.sha256"

expected="$(cut -d ' ' -f 1 < "$work/$archive.sha256")"
if command -v sha256sum > /dev/null; then
    actual="$(sha256sum "$work/$archive" | cut -d ' ' -f 1)"
else
    actual="$(shasum -a 256 "$work/$archive" | cut -d ' ' -f 1)"
fi
if [ "$expected" != "$actual" ]; then
    echo "splice install: checksum mismatch for $archive" >&2
    exit 1
fi

tar -xzf "$work/$archive" -C "$work"
mkdir -p "$install_dir"
install -m 755 "$work/splice-$target/splice" "$install_dir/splice"
echo "splice install: $("$install_dir/splice" --version) installed to $install_dir/splice"
case ":$PATH:" in
    *":$install_dir:"*) ;;
    *) echo "splice install: $install_dir is not on PATH; add it to use splice and its hook" >&2 ;;
esac
