#!/usr/bin/env bash
# Import dashboard JSON files from dashboards/ into OpenObserve.
# Skips dashboards that already exist (matched by title).
set -euo pipefail

: "${ZO_ROOT_USER_EMAIL:=dev@o2.test}"
: "${ZO_ROOT_USER_PASSWORD:=dev}"
: "${ZO_HTTP_PORT:=5080}"

BASE="http://localhost:${ZO_HTTP_PORT}"
ORG="default"
AUTH=$(printf '%s:%s' "$ZO_ROOT_USER_EMAIL" "$ZO_ROOT_USER_PASSWORD")

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DASHBOARDS_DIR="${SCRIPT_DIR}/../docs/o2-dashboards"

if [ ! -d "$DASHBOARDS_DIR" ]; then
  echo "No dashboards/ directory found, skipping import."
  exit 0
fi

imported=0
skipped=0

for f in "$DASHBOARDS_DIR"/*.json; do
  [ -f "$f" ] || continue

  title=$(python3 -c "import json,sys; print(json.load(sys.stdin)['title'])" < "$f" 2>/dev/null || echo "")
  if [ -z "$title" ]; then
    echo "  SKIP $(basename "$f"): no title field"
    skipped=$((skipped + 1))
    continue
  fi

  # List existing dashboards and check if one with the same title exists.
  exists=$(curl -sf -u "$AUTH" "${BASE}/api/${ORG}/dashboards" 2>/dev/null \
    | python3 -c "
import json, sys
data = json.load(sys.stdin)
dashboards = data.get('dashboards', [])
print(any(d.get('title') == '$title' for d in dashboards))
" 2>/dev/null || echo "False")

  if [ "$exists" = "True" ]; then
    echo "  SKIP $(basename "$f"): \"${title}\" already exists"
    skipped=$((skipped + 1))
    continue
  fi

  status=$(curl -sf -o /dev/null -w '%{http_code}' \
    -u "$AUTH" \
    -H 'Content-Type: application/json' \
    -X POST "${BASE}/api/${ORG}/dashboards" \
    -d @"$f" 2>/dev/null || echo "000")

  if [ "$status" = "200" ]; then
    echo "  OK   $(basename "$f"): \"${title}\""
    imported=$((imported + 1))
  else
    echo "  FAIL $(basename "$f"): HTTP ${status}"
  fi
done

echo "Dashboard import: ${imported} imported, ${skipped} skipped."
