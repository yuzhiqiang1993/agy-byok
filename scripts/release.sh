#!/usr/bin/env bash

# AGY BYOK 发布脚本。
# 功能代码应先独立提交；本脚本只处理版本元数据、发布日志、标签和 GitHub Release。

set -Eeuo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
cd -- "$ROOT_DIR"
readonly RELEASE_BRANCH="main"
readonly RELEASE_WORKFLOW="release.yml"
readonly TAG_PREFIX="app-v"
VERSION=""
NOTES_FILE=""
AUTO_APPROVE=0
DRY_RUN=0
PREPARE_ONLY=0
WAIT_FOR_RELEASE=1
LOCAL_MACOS_BUILD=0
RELEASE_BODY_FILE=""
usage() {
  cat <<'USAGE'
用法：
  scripts/release.sh <版本号> --notes-file <发布说明文件> [选项]

示例：
  scripts/release.sh 1.1.4 --notes-file /tmp/agy-byok-1.1.4.md
  scripts/release.sh 1.1.4 --notes-file /tmp/agy-byok-1.1.4.md --yes

选项：
  --notes-file <文件>       发布说明正文；会同时写入 CHANGELOG.md 和 GitHub Release
  --yes                     跳过发布前确认，适合 AI 或自动化调用
  --dry-run                 只执行前置检查并展示动作，不修改文件、不提交、不推送
  --prepare-only            更新版本和 CHANGELOG、运行检查后停止，不提交、不推送
  --skip-wait               推送后不等待 GitHub Actions 完成
  --local-macos-build       发布前额外构建未签名的 macOS Apple Silicon 本地产物
  -h, --help                显示帮助

发布说明文件只写正文，例如：
  ### 问题修复

  - 修复……
USAGE
}
die() {
  printf '错误：%s\n' "$*" >&2
  exit 1
}
info() {
  printf '\n==> %s\n' "$*"
}
warn() {
  printf '警告：%s\n' "$*" >&2
}
run_step() {
  local description="$1"
  shift
  info "$description"
  printf '+ '
  printf '%q ' "$@"
  printf '\n'
  "$@"
}
require_command() {
  command -v "$1" >/dev/null 2>&1 || die "缺少命令：$1"
}
cleanup() {
  if [[ -n "${RELEASE_BODY_FILE:-}" && -f "$RELEASE_BODY_FILE" ]]; then
    rm -f -- "$RELEASE_BODY_FILE"
  fi
}
trap cleanup EXIT

while (($# > 0)); do
  case "$1" in
    --notes-file)
      (($# >= 2)) || die "--notes-file 需要文件路径"
      NOTES_FILE="$2"
      shift 2
      ;;
    --yes)
      AUTO_APPROVE=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --prepare-only)
      PREPARE_ONLY=1
      shift
      ;;
    --skip-wait)
      WAIT_FOR_RELEASE=0
      shift
      ;;
    --local-macos-build)
      LOCAL_MACOS_BUILD=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --*)
      die "未知选项：$1"
      ;;
    *)
      [[ -z "$VERSION" ]] || die "只能指定一个版本号：$VERSION / $1"
      VERSION="$1"
      shift
      ;;
  esac
done

[[ -n "$VERSION" ]] || { usage >&2; die "缺少版本号"; }
[[ -n "$NOTES_FILE" ]] || die "必须提供 --notes-file"
[[ -f "$NOTES_FILE" ]] || die "发布说明文件不存在：$NOTES_FILE"
[[ -s "$NOTES_FILE" ]] || die "发布说明文件为空：$NOTES_FILE"

[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
  || die "版本号必须是稳定 SemVer，例如 1.1.4：$VERSION"

for command_name in git node npm cargo python3 gh; do
  require_command "$command_name"
done

CURRENT_VERSION="$(node -p "require('./package.json').version")"
python3 - "$CURRENT_VERSION" "$VERSION" <<'PY'
import sys

current = tuple(int(part) for part in sys.argv[1].split("."))
target = tuple(int(part) for part in sys.argv[2].split("."))
if target <= current:
    raise SystemExit(f"目标版本必须高于当前版本 {sys.argv[1]}：{sys.argv[2]}")
PY

TAG="${TAG_PREFIX}${VERSION}"

CURRENT_BRANCH="$(git branch --show-current)"
[[ "$CURRENT_BRANCH" == "$RELEASE_BRANCH" ]] \
  || die "必须从 $RELEASE_BRANCH 分支发布，当前分支：$CURRENT_BRANCH"

[[ -z "$(git status --porcelain=v1)" ]] \
  || die "工作树不干净；请先提交功能代码，发布脚本只处理版本与发布元数据"

git remote get-url origin >/dev/null 2>&1 \
  || die "未配置 origin 远程仓库"

run_step "检查 GitHub CLI 登录状态" gh auth status
run_step "同步 main 与远程标签" git fetch origin "$RELEASE_BRANCH" --tags

git merge-base --is-ancestor "origin/$RELEASE_BRANCH" HEAD \
  || die "本地 $RELEASE_BRANCH 落后于 origin/$RELEASE_BRANCH，请先同步远程提交"

if git rev-parse --verify --quiet "refs/tags/$TAG" >/dev/null \
  || git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then
  die "版本标签已存在，不能重复发布：$TAG"
fi

python3 - "$NOTES_FILE" "$VERSION" <<'PY'
import re
import sys
from pathlib import Path

notes = Path(sys.argv[1]).read_text(encoding="utf-8").strip()
version = sys.argv[2]
if not notes:
    raise SystemExit("发布说明不能为空")
if re.search(rf"^## \[{re.escape(version)}\]", notes, re.MULTILINE):
    raise SystemExit("发布说明文件不应重复包含版本标题，脚本会自动生成标题")
PY

if ((DRY_RUN)); then
  info "Dry-run：前置检查通过"
  printf '%s\n' \
    "目标版本：$VERSION" \
    "发布标签：$TAG" \
    "将更新：package.json、package-lock.json、三个 Cargo 包、Cargo.lock、tauri.conf.json、CHANGELOG.md" \
    "将提交：chore(release): 准备 v$VERSION" \
    "将推送：main 和 $TAG" \
    "将等待：GitHub Actions $RELEASE_WORKFLOW"
  exit 0
fi

if ((AUTO_APPROVE == 0)); then
  printf '\n即将发布 %s：更新版本、修改 CHANGELOG、提交、创建标签并推送到 GitHub。\n' "$VERSION"
  printf '确认请输入 RELEASE： '
  read -r confirmation
  [[ "$confirmation" == "RELEASE" ]] || die "已取消发布"
fi

update_versions() {
  info "同步项目版本号：$VERSION"
  VERSION_TO_WRITE="$VERSION" ROOT_TO_WRITE="$ROOT_DIR" python3 - <<'PY'
import os
import re
from pathlib import Path

root = Path(os.environ["ROOT_TO_WRITE"])
version = os.environ["VERSION_TO_WRITE"]

def replace_json_versions(relative_path: str, expected_count: int) -> None:
    path = root / relative_path
    text = path.read_text(encoding="utf-8")
    updated, count = re.subn(
        r'(?m)^([ \t]*"version"[ \t]*:[ \t]*")[^"]+(")',
        rf'\g<1>{version}\g<2>',
        text,
        count=expected_count,
    )
    if count != expected_count:
        raise SystemExit(f"未能更新 JSON 版本：{relative_path}")
    path.write_text(updated, encoding="utf-8")

replace_json_versions("package.json", 1)
replace_json_versions("package-lock.json", 2)
replace_json_versions("src-tauri/tauri.conf.json", 1)

for relative_path in (
    "src-tauri/Cargo.toml",
    "crates/proxy-core/Cargo.toml",
    "crates/host-integration/Cargo.toml",
):
    path = root / relative_path
    text = path.read_text(encoding="utf-8")
    updated, count = re.subn(
        r'(?m)^(version[ \t]*=[ \t]*")[^"]+("[ \t]*)$',
        rf'\g<1>{version}\g<2>',
        text,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"未能更新 Cargo 包版本：{relative_path}")
    path.write_text(updated, encoding="utf-8")

lock_path = root / "Cargo.lock"
lock_text = lock_path.read_text(encoding="utf-8")
for package_name in ("agy-byok", "agy-byok-desktop", "host-integration"):
    pattern = re.compile(
        rf'(\[\[package\]\]\nname = "{re.escape(package_name)}"\nversion = ")[^"]+(")'
    )
    lock_text, count = pattern.subn(
        rf"\g<1>{version}\g<2>",
        lock_text,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"未能更新 Cargo.lock 包版本：{package_name}")
lock_path.write_text(lock_text, encoding="utf-8")
PY
}

update_changelog() {
  info "写入 CHANGELOG.md：$VERSION"
  NOTES_TO_WRITE="$NOTES_FILE" VERSION_TO_WRITE="$VERSION" ROOT_TO_WRITE="$ROOT_DIR" python3 - <<'PY'
import os
import re
from datetime import date
from pathlib import Path

root = Path(os.environ["ROOT_TO_WRITE"])
version = os.environ["VERSION_TO_WRITE"]
notes = Path(os.environ["NOTES_TO_WRITE"]).read_text(encoding="utf-8").strip()
path = root / "CHANGELOG.md"
text = path.read_text(encoding="utf-8")

if re.search(rf"^## \[{re.escape(version)}\]", text, re.MULTILINE):
    raise SystemExit(f"CHANGELOG.md 已存在版本：{version}")
marker = "# 更新日志"
if marker not in text:
    raise SystemExit("CHANGELOG.md 缺少顶部标题：# 更新日志")

section = f"## [{version}] - {date.today().isoformat()}\n\n{notes}\n"
updated = text.replace(marker, f"{marker}\n\n{section}", 1)
path.write_text(updated, encoding="utf-8")
PY
}

assert_versions() {
  EXPECTED_VERSION="$VERSION" node - <<'NODE'
const fs = require("node:fs");
const expected = process.env.EXPECTED_VERSION;

for (const file of ["package.json", "package-lock.json", "src-tauri/tauri.conf.json"]) {
  const data = JSON.parse(fs.readFileSync(file, "utf8"));
  const actual = file === "package-lock.json" ? data.packages[""].version : data.version;
  if (actual !== expected) {
    throw new Error(file + " 版本不一致：" + actual + " != " + expected);
  }
}
NODE
}

update_versions
update_changelog
assert_versions

run_step "安装锁定的前端依赖" npm ci
run_step "校验 i18n" npm run check:i18n
run_step "校验 TypeScript" npx tsc --noEmit
run_step "校验前端生产构建" npm run build
run_step "校验 Rust 格式" cargo fmt --all -- --check
run_step "运行 Rust Clippy 静态检查" cargo clippy --workspace --all-targets --locked -- -D warnings
run_step "运行 Rust 工作区测试" cargo test --workspace --locked
run_step "校验 Cargo.lock" sh -c 'cargo metadata --locked --no-deps --format-version 1 >/dev/null'
run_step "校验差异空白" git diff --check

HARNESS_SCRIPT="${HARNESS_SCRIPT:-$ROOT_DIR/../ai-harness-kit/scripts/verify-harness.sh}"
if [[ -f "$HARNESS_SCRIPT" ]]; then
  run_step "运行 Harness quick 自检" bash "$HARNESS_SCRIPT" --quick
else
  warn "未找到 Harness quick 脚本，已跳过：$HARNESS_SCRIPT"
fi

if ((LOCAL_MACOS_BUILD)); then
  [[ "$(uname -s)" == "Darwin" ]] || die "--local-macos-build 只能在 macOS 上执行"
  run_step "构建未签名 macOS Apple Silicon 本地产物" \
    npm run tauri build -- \
      --target aarch64-apple-darwin \
      --config src-tauri/tauri.macos.conf.json \
      --no-sign
fi

if ((PREPARE_ONLY)); then
  info "prepare-only：版本与发布日志已准备，未提交、未创建标签、未推送"
  git status --short
  exit 0
fi

RELEASE_FILES=(
  CHANGELOG.md
  Cargo.lock
  crates/host-integration/Cargo.toml
  crates/proxy-core/Cargo.toml
  package-lock.json
  package.json
  src-tauri/Cargo.toml
  src-tauri/tauri.conf.json
)

git add -- "${RELEASE_FILES[@]}"
git diff --cached --check

EXPECTED_STAGED="$(printf '%s\n' "${RELEASE_FILES[@]}" | sort)"
ACTUAL_STAGED="$(git diff --cached --name-only | sort)"
[[ "$EXPECTED_STAGED" == "$ACTUAL_STAGED" ]] \
  || die "暂存区包含非发布文件，请检查：\n$ACTUAL_STAGED"

run_step "提交发布元数据" git commit -m "chore(release): 准备 v$VERSION"
run_step "创建发布标签" git tag -a "$TAG" -m "release: v$VERSION"
run_step "推送 main 与发布标签" git push origin "$RELEASE_BRANCH" --follow-tags

if ((WAIT_FOR_RELEASE == 0)); then
  printf '\n已推送 %s；未等待 GitHub Actions。\n' "$TAG"
  exit 0
fi

info "等待 GitHub Actions 发布工作流"
HEAD_SHA="$(git rev-parse HEAD)"
RUN_ID=""
WAIT_SECONDS=600
ELAPSED=0
while ((ELAPSED < WAIT_SECONDS)); do
  RUN_ID="$(gh run list \
    --workflow "$RELEASE_WORKFLOW" \
    --limit 20 \
    --json databaseId,headSha,event \
    --jq ".[] | select(.headSha == \"$HEAD_SHA\" and .event == \"push\") | .databaseId" \
    | head -n 1)"
  if [[ -n "$RUN_ID" ]]; then
    break
  fi
  sleep 5
  ((ELAPSED += 5))
done

[[ -n "$RUN_ID" ]] || die "在 ${WAIT_SECONDS}s 内未找到对应的 GitHub Actions 运行记录"
run_step "等待 GitHub Actions #$RUN_ID" gh run watch "$RUN_ID" --exit-status

RELEASE_BODY_FILE="$(mktemp "${TMPDIR:-/tmp}/agy-byok-release.XXXXXX")"
{
  cat -- "$NOTES_FILE"
  printf '\n\n### Downloads\n\n'
  printf '%s\n' \
    '- **macOS Apple Silicon (M-series)**: Download the DMG containing `macOS-Apple-Silicon`.' \
    '- **macOS Intel**: Download the DMG containing `macOS-Intel`.' \
    '- **Windows 64-bit**: Download the `x64` setup EXE.' \
    '- Other assets are used for in-app automatic updates and do not need to be downloaded manually.'
} > "$RELEASE_BODY_FILE"

run_step "同步 GitHub Release 正文" gh release edit "$TAG" \
  --title "AGY BYOK v$VERSION" \
  --notes-file "$RELEASE_BODY_FILE"

run_step "核验 GitHub Release" gh release view "$TAG" \
  --json name,tagName,isDraft,isPrerelease,assets,url \
  --jq '{name, tagName, isDraft, isPrerelease, assetCount: (.assets | length), url}'

printf '\n发布完成：%s\n' "https://github.com/yuzhiqiang1993/agy-byok/releases/tag/$TAG"
