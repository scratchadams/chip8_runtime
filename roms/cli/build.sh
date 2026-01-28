#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

mkdir -p "$root_dir/build"

cat "$root_dir/cli.c8s" \
    "$root_dir/lib/sys.c8s" \
    "$root_dir/lib/data.c8s" \
    > "$root_dir/build/cli_combined.c8s"

cargo run --quiet --bin c8asm -- \
    "$root_dir/build/cli_combined.c8s" \
    "$root_dir/build/cli.ch8"

echo "Wrote $root_dir/build/cli.ch8"
