#!/usr/bin/env bash
# The API contract (docs/adr/0007-generated-api-contract.md): the OpenAPI document from the Rust
# types, and the frontend's TypeScript types from that document.
#   scripts/gen-api.sh write   regenerate both (run after changing a route or an API type)
#   scripts/gen-api.sh check   fail if either differs from what's committed
set -euo pipefail

mode=${1:?usage: gen-api.sh write|check}
doc=web/src/api/openapi.json
types=web/src/api/generated/schema.ts
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cargo run --quiet --locked -p api --example openapi >"$tmp/openapi.json"
npm --prefix web exec --offline -- openapi-typescript "$tmp/openapi.json" -o "$tmp/schema.ts" >/dev/null

case $mode in
  write)
    mkdir -p "$(dirname "$types")"
    cp "$tmp/openapi.json" "$doc"
    cp "$tmp/schema.ts" "$types"
    ;;
  check)
    status=0
    diff -u --label "$doc (committed)" --label "$doc (generated)" "$doc" "$tmp/openapi.json" || status=1
    diff -u --label "$types (committed)" --label "$types (generated)" "$types" "$tmp/schema.ts" || status=1
    if [[ $status -ne 0 ]]; then
      echo "gen-api: the API contract is out of date; run \`just gen-api\` and commit the result" >&2
    fi
    exit $status
    ;;
  *) echo "usage: gen-api.sh write|check" >&2; exit 2 ;;
esac
