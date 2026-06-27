#!/usr/bin/env bash
# Copy the starter workspace (sample CSVs + pipelines) into a target folder.
set -euo pipefail

TARGET="${1:-}"
if [[ -z "$TARGET" ]]; then
  echo "Usage: $0 <workspace-dir>" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/examples/starter-workspace"

mkdir -p "$TARGET"/{data,output,pipelines,logs}
cp "$ROOT/samples/orders.csv" "$ROOT/samples/regions.csv" "$TARGET/data/"
cp "$SRC/pipelines/"*.json "$TARGET/pipelines/"

# Pipelines reference paths via the built-in ${workspace} placeholder, so no
# repository.json / contexts/ are needed for the starter set.

echo "Starter workspace seeded at $TARGET"
echo "  data/orders.csv, data/regions.csv"
echo "  pipelines/orders_filter.pipeline.json"
echo "  pipelines/region_summary.pipeline.json"
