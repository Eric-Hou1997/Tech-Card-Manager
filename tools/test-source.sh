#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/tech-card-manager-test.XXXXXX")
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

python3 "$ROOT/tools/build-language-packs.py"

for test_file in "$ROOT"/windows/tests/*.py; do python3 "$test_file"; done
node -e 'const fs=require("fs"); const html=fs.readFileSync(process.argv[1], "utf8"); for (const script of html.matchAll(/<script[^>]*>([\s\S]*?)<\/script>/gi)) new Function(script[1]);' "$ROOT/windows/web/index.html"
node --check "$ROOT/windows/engine/technical-specs-card.js"
node "$ROOT/windows/tests/i18n_runtime_regression.js"
node "$ROOT/windows/tests/responsive_layout_regression.js"

(
  cd "$ROOT/windows"
  GOCACHE="$TMP/go-cache" GOMODCACHE="$TMP/go-mod-cache" GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go test -c -o "$TMP/Windows-Go-Tests.exe" .
  GOCACHE="$TMP/go-cache" GOMODCACHE="$TMP/go-mod-cache" GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go vet ./...
  GOCACHE="$TMP/go-cache" GOMODCACHE="$TMP/go-mod-cache" GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -ldflags="-s -w -H windowsgui" -o "$TMP/Tech-Card-Manager.exe" .
)

echo "OK Tech Card Manager v4.1.0 source, Python contracts, JavaScript syntax, Go tests, vet, and Windows x64 cross-build"
