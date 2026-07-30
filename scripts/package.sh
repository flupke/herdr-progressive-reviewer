#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
cargo build --release --locked --bins

version=$(sed -n 's/^version = "\(.*\)"/\1/p' herdr-plugin.toml)
platform=$(uname -s | tr '[:upper:]' '[:lower:]')
architecture=$(uname -m)
name="herdr-progressive-reviewer-${version}-${platform}-${architecture}"
package="dist/${name}"

mkdir -p "${package}/bin"
cp herdr-plugin.toml README.md RELEASE_NOTES.md "${package}/"
cp target/release/pr-app target/release/pr-control "${package}/bin/"
tar -C dist -czf "dist/${name}.tar.gz" "${name}"
printf '%s\n' "dist/${name}.tar.gz"
