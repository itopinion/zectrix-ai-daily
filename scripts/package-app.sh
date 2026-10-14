#!/bin/bash
# Build an installable Hub ZIP. The supported target is macOS Apple Silicon,
# matching the vocabulary app. Other hosts can still emit a contract-check
# archive when ZECTRIX_ALLOW_HOST_PACKAGE=1; that archive is not a Note 4 build.
set -euo pipefail
PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_DIR"

os="$(uname -s)"
arch="$(uname -m)"
if [[ "$os" == Darwin && "$arch" == arm64 ]]; then
  PLATFORM="macos-aarch64"
elif [[ "${ZECTRIX_ALLOW_HOST_PACKAGE:-0}" == 1 ]]; then
  case "$os-$arch" in
    Linux-x86_64) PLATFORM="linux-x86_64" ;;
    Linux-aarch64|Linux-arm64) PLATFORM="linux-aarch64" ;;
    Darwin-x86_64) PLATFORM="macos-x86_64" ;;
    *) echo "不支持的打包平台：$os-$arch" >&2; exit 1 ;;
  esac
  echo "警告：当前生成的是 $PLATFORM 契约检查包，不是 Hub 首版目标 macos-aarch64。" >&2
else
  echo "首版应用包仅支持 macOS Apple Silicon。在其他系统上检查 ZIP 契约时请设置 ZECTRIX_ALLOW_HOST_PACKAGE=1。" >&2
  exit 1
fi

if [[ "${SKIP_BUILD:-0}" != 1 ]]; then
  "${CARGO:-cargo}" build --release --locked
fi
BINARY="${AI_DAILY_BINARY:-${CARGO_TARGET_DIR:-$PROJECT_DIR/target}/release/zectrix-ai-daily}"
if [[ "$os" == Darwin ]]; then
  [[ -x "$BINARY" ]] || { echo "未找到可执行程序：$BINARY" >&2; exit 1; }
else
  [[ -f "$BINARY" ]] || { echo "未找到可执行程序：$BINARY" >&2; exit 1; }
fi

mkdir -p "$PROJECT_DIR/dist"
STAGE="$(mktemp -d "$PROJECT_DIR/dist/app-stage.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/bin" "$STAGE/workers"
cp "$BINARY" "$STAGE/bin/zectrix-ai-daily"
cp "$PROJECT_DIR"/workers/*.py "$STAGE/workers/"
cp "$PROJECT_DIR/sources.toml" "$STAGE/sources.toml"
for ITEM in README.md README.en.md LICENSE THIRD_PARTY_NOTICES.md app.json; do
  cp "$PROJECT_DIR/$ITEM" "$STAGE/$ITEM"
done
"${PYTHON:-python3}" "$PROJECT_DIR/scripts/package-app.py" "$STAGE" "$PROJECT_DIR/dist" "$PLATFORM"
