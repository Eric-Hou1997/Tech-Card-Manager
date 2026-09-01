#!/bin/sh
set -eu

# Tech Card Manager v4.0.0: package the reviewed portable x64 GUI as ZIP only.
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
OUT="$ROOT/releases"
TMP=$(mktemp -d "${TMPDIR:-/tmp}/tech-card-manager-win-4.0.0.XXXXXX")
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
GO_CACHE="$TMP/go-cache"
GO_MOD_CACHE="$TMP/go-mod-cache"
WIN_EXE_NAME="Tech-Card-Manager-Windows-x64-v4.0.0.exe"
WIN_ZIP_NAME="Tech-Card-Manager-Windows-x64-v4.0.0.zip"
SHA_NAME="SHA256SUMS-Tech-Card-Manager-Windows-v4.0.0.txt"
README_NAME="README-Windows-v4.0.0.txt"
CHANGELOG_NAME="CHANGELOG-Windows-v4.0.0.txt"

for target in "$OUT/$WIN_ZIP_NAME" "$OUT/$SHA_NAME"; do
  if [ -e "$target" ]; then
    echo "refusing to overwrite existing release target: $target" >&2
    exit 2
  fi
done

grep -Fq 'const appVersion = "4.0.0"' "$ROOT/windows/main.go"
grep -Fq "\$ManagerVersion = '4.0.0'" "$ROOT/windows/engine/windows-engine.ps1"
grep -Fq 'const WEB_CARD_VERSION = "4.0.0"' "$ROOT/windows/engine/technical-specs-card.js"
git -C "$ROOT" diff --check

mkdir -p "$OUT" "$TMP/windows"
sh "$ROOT/tools/test-source.sh"

(
  cd "$ROOT/windows"
  GOCACHE="$GO_CACHE" GOMODCACHE="$GO_MOD_CACHE" GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go test -c -o "$TMP/windows/Windows-Go-Tests.exe" .
  GOCACHE="$GO_CACHE" GOMODCACHE="$GO_MOD_CACHE" GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go vet ./...
  GOCACHE="$GO_CACHE" GOMODCACHE="$GO_MOD_CACHE" GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -ldflags="-s -w -H windowsgui" -o "$TMP/windows/$WIN_EXE_NAME" .
)

cp "$ROOT/packaging/README.txt" "$TMP/windows/README.txt"
cp "$ROOT/packaging/CHANGELOG.txt" "$TMP/windows/CHANGELOG.txt"
(
  cd "$TMP/windows"
  zip -q "$OUT/$WIN_ZIP_NAME" "$WIN_EXE_NAME" README.txt CHANGELOG.txt
  unzip -t "$OUT/$WIN_ZIP_NAME" >/dev/null
  [ "$(unzip -Z1 "$OUT/$WIN_ZIP_NAME" | sort)" = "$(printf '%s\n' "$WIN_EXE_NAME" CHANGELOG.txt README.txt | sort)" ]
)
(
  cd "$OUT"
  shasum -a 256 "$WIN_ZIP_NAME" > "$SHA_NAME"
  shasum -a 256 -c "$SHA_NAME"
)
if find "$OUT" -maxdepth 1 -type f -name '*.exe' -print -quit | grep -q .; then
  echo "error: bare .exe found in releases/" >&2
  exit 3
fi
file "$TMP/windows/$WIN_EXE_NAME"
echo "Tech Card Manager v4.0.0 Windows 发布文件已生成（仅 ZIP）：$OUT/$WIN_ZIP_NAME"
