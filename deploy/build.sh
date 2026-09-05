#!/bin/sh
# What a host runs to turn a checkout into something to serve: the browser
# client first, then the server that serves it. Run it from anywhere -- it
# builds the checkout it is in.
#
# Both are the shipping builds. `wasm-pack build --target web` runs wasm-opt
# over the whole module, which is most of the two minutes and takes it from
# 12.1 MB to 7.5 MB; `--profiling` skips it and is for iterating, not for this.
# See docs/README.md.
#
# It stops at the first failure, and that is the point of the file: a wasm
# build that fails leaves the *old* pkg/ in place, so a deploy that carried on
# would serve an old module against a new page and nothing would say so.
set -eu

cd "$(dirname "$0")/.."
root=$(pwd)

if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "no wasm-pack: cargo install wasm-pack" >&2
    exit 1
fi

echo "== the browser client, in $root"
wasm-pack build --target web

echo "== the server"
cargo build --release --no-default-features --features server --bin server

# Printed because they are the two numbers worth noticing between deploys: the
# module is what every visitor downloads, and a binary that did not change size
# at all is usually one that did not get rebuilt.
size() {
    wc -c <"$1" | awk '{ printf "%6.1f MB", $1 / 1048576 }'
}

wasm=pkg/conwayskingdom_bg.wasm
# Where cargo actually put it, which is `target` unless somebody shares one
# between checkouts.
server="${CARGO_TARGET_DIR:-target}/release/server"
echo
echo "$(size "$wasm")  $wasm"
echo "$(size "$server")  $server"
