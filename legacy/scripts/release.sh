#!/bin/bash
# ---------------------------------------------------------------------------
# Cut a release from this machine, without GitHub Actions.
#
#   ./scripts/release.sh --dry-run      build and package, then stop: no
#                                       commit, no push, no upload
#   ./scripts/release.sh 0.2.0          bump, build, tag, push, upload
#   ./scripts/release.sh                release whatever version the repo
#                                       already says (electron/package.json)
#   ./scripts/release.sh --draft        open the release as a draft; publish it
#                                       by hand once the dmg has been tried
#   ./scripts/release.sh --skip-build   reuse the .dmg/.zip already sitting in
#                                       electron/release (a retry after the
#                                       upload failed, say)
#
# What it does, in order, stopping at the first failure:
#
#   1. checks the ground: gh installed and logged in, on the release branch,
#      work tree clean, and the tag free -- or already naming this exact commit,
#      which is a half-finished release being resumed
#   2. writes the version into the two files that must both carry it:
#      electron/package.json (npm and electron-builder read it) and
#      cmake/Version.cmake   (the C++ core, and the libretro core's
#                             library_version)
#   3. ./wasm/build.sh                 the C++ core, as WebAssembly
#      pnpm run build                  main process, renderer, gamepad helper
#   4. electron-builder                .dmg and .zip into electron/release/
#      shasum                           SHA256SUMS.txt beside them
#   5. commit the version, tag it v<version>, push both
#   6. gh release create               notes from the commits since the last
#                                      tag, every artifact attached
#
# `--dry-run` does 1 through 4 for real -- the point is to find out whether the
# packaging works -- and then stops. It writes the version to both files so the
# file names are right, and puts them back on the way out, so it cannot leave
# the tree dirty.
#
# There is no signing here, on purpose: the dmg is ad-hoc and Gatekeeper will
# quarantine it. The release notes tell the player to run `xattr` once, and the
# README says the same. If that ever changes, it changes in `build.mac.identity`
# in electron/package.json, not in this script.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ELECTRON="$ROOT/electron"
PKG="$ELECTRON/package.json"
# The version has to live in two places -- package.json because npm and
# electron-builder read it there, and cmake/Version.cmake because CMake does --
# and a release that updates only one of them ships a core whose
# library_version disagrees with the app. This script writes both and treats a
# disagreement between them as a version to be fixed, not as a no-op.
CMAKE_VERSION_FILE="$ROOT/cmake/Version.cmake"
OUT="$ELECTRON/release"
BRANCH="main"
ARCH="arm64"

VERSION=""
DRAFT=0
PRERELEASE=0
DRY_RUN=0
SKIP_WASM=0
SKIP_BUILD=0
ALLOW_DIRTY=0
NOTES_FILE=""

PKG_BACKUP=""
CMAKE_VERSION_BACKUP=""
NOTES=""

cleanup() {
    if [ -n "$PKG_BACKUP" ] && [ -f "$PKG_BACKUP" ]; then
        cp "$PKG_BACKUP" "$PKG"
        rm -f "$PKG_BACKUP"
    fi
    if [ -n "$CMAKE_VERSION_BACKUP" ] && [ -f "$CMAKE_VERSION_BACKUP" ]; then
        cp "$CMAKE_VERSION_BACKUP" "$CMAKE_VERSION_FILE"
        rm -f "$CMAKE_VERSION_BACKUP"
    fi
    if [ -n "$NOTES" ] && [ -f "$NOTES" ]; then
        rm -f "$NOTES"
    fi
    return 0
}
trap cleanup EXIT

usage() {
    awk 'NR > 1 && /^set -euo/ { exit } NR > 1 { sub(/^# ?/, ""); print }' "$0"
    exit "${1:-0}"
}

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
note() { printf '    %s\n' "$*"; }

# The version as cmake/Version.cmake currently spells it.
cmake_version() {
    sed -n 's/.*set(FC_PROJECT_VERSION "\([^"]*\)").*/\1/p' "$CMAKE_VERSION_FILE"
}

write_version() {
    PKG="$PKG" VERSION="$VERSION" node -e '
        const fs = require("fs");
        const file = process.env.PKG;
        const json = JSON.parse(fs.readFileSync(file, "utf8"));
        json.version = process.env.VERSION;
        fs.writeFileSync(file, JSON.stringify(json, null, 2) + "\n");
    '

    # The CMake copy. Rewritten as text, and a failed substitute is an error
    # rather than a silent no-op: a release that ships a core reporting the
    # wrong library_version is a bug report nobody can act on.
    CMAKE_VERSION_FILE="$CMAKE_VERSION_FILE" VERSION="$VERSION" node -e '
        const fs = require("fs");
        const file = process.env.CMAKE_VERSION_FILE;
        const text = fs.readFileSync(file, "utf8");
        const next = text.replace(
            /set\(FC_PROJECT_VERSION "[^"]*"\)/,
            `set(FC_PROJECT_VERSION "${process.env.VERSION}")`);
        if (next === text) {
            console.error(`no set(FC_PROJECT_VERSION ...) in ${file}`);
            process.exit(1);
        }
        fs.writeFileSync(file, next);
    '
}

# --- arguments -------------------------------------------------------------
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)        usage 0 ;;
        --draft)          DRAFT=1 ;;
        --prerelease)     PRERELEASE=1 ;;
        --dry-run)        DRY_RUN=1 ;;
        --skip-wasm)      SKIP_WASM=1 ;;
        --skip-build)     SKIP_BUILD=1; SKIP_WASM=1 ;;
        --allow-dirty)    ALLOW_DIRTY=1 ;;
        --arch)           ARCH="${2:-}"; [ -n "$ARCH" ] || die "--arch needs a value"; shift ;;
        --branch)         BRANCH="${2:-}"; [ -n "$BRANCH" ] || die "--branch needs a value"; shift ;;
        --notes)          NOTES_FILE="${2:-}"; [ -n "$NOTES_FILE" ] || die "--notes needs a file"; shift ;;
        -*)               die "unknown option $1 (try --help)" ;;
        *)                [ -z "$VERSION" ] || die "version given twice"; VERSION="$1" ;;
    esac
    shift
done

[ -f "$PKG" ] || die "not a checkout of this project: $PKG is missing"
[ -f "$CMAKE_VERSION_FILE" ] || die "not a checkout of this project: $CMAKE_VERSION_FILE is missing"

# --- preflight -------------------------------------------------------------
step "Checking the ground"

for tool in git node pnpm gh shasum; do
    command -v "$tool" >/dev/null 2>&1 || die "$tool is not on PATH"
done
gh auth status >/dev/null 2>&1 || die "gh is not logged in; run: gh auth login"

# --- what the app is called -------------------------------------------------
# One source of truth: electron/package.json. Everything a player sees -- the
# release title, the .dmg file name, the line that says which folder to drag
# into Applications, the quarantine command -- is spelled once, here, and
# derived from build.productName. Renaming the app is a one-line change in
# package.json; nothing in this script has to be found and edited to match.
#
# APP_NAME   display name  "Classic Game Box"   productName, window title, dmg
# APP_SLUG   file names    "classic-game-box"   package.json name, temp files
# APP_ID     bundle id     "com.….electron"     appId
APP_NAME="$(node -p "require('$PKG').build.productName")"
APP_ID="$(node -p "require('$PKG').build.appId")"
APP_SLUG="$(node -p "require('$PKG').name")"
[ -n "$APP_NAME" ] || die "build.productName is missing from $PKG"
[ -n "$APP_SLUG" ] || die "name is missing from $PKG"
# The display name goes straight into a file name and into a glob, so a slash,
# quote or colon in it turns into a build that produces nothing and a `find`
# that matches nothing. Say so here rather than three steps later.
printf '%s' "$APP_NAME" | grep -qE '^[^/\\:*?"<>|]+$' \
    || die "build.productName '$APP_NAME' has characters a file name cannot hold"

CURRENT_BRANCH="$(git -C "$ROOT" rev-parse --abbrev-ref HEAD)"
[ "$CURRENT_BRANCH" = "$BRANCH" ] || die "on '$CURRENT_BRANCH', expected '$BRANCH' (--branch to change)"

if [ "$DRY_RUN" = 0 ]; then
    if [ -n "$(git -C "$ROOT" status --porcelain)" ]; then
        if [ "$ALLOW_DIRTY" = 1 ]; then
            note "work tree is dirty, --allow-dirty given"
        else
            git -C "$ROOT" status --short >&2
            die "work tree is dirty; commit or stash first (--allow-dirty to override)"
        fi
    fi
fi

CURRENT_VERSION="$(node -p "require('$PKG').version")"
CURRENT_CMAKE_VERSION="$(cmake_version)"
[ -n "$VERSION" ] || VERSION="$CURRENT_VERSION"
# Keep the version a version: electron-builder reads it and the tag is built
# from it, so a stray "v" or space ends up in a file name.
VERSION="${VERSION#v}"
printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+' \
    || die "version '$VERSION' is not x.y.z"

TAG="v$VERSION"
TITLE="$APP_NAME $VERSION"
# Every name electron-builder will produce, and every name the notes mention,
# derived once so the three cannot drift apart. electron-builder names its
# artifacts "${productName}-${version}-${arch}.${ext}" by default.
ARTIFACT_GLOB="$APP_NAME-$VERSION-*"
DMG_NAME="$APP_NAME-$VERSION-$ARCH.dmg"
APP_BUNDLE="$APP_NAME.app"

# Three states, and the difference between them is what makes a retry possible:
# nothing tagged yet (the normal run), the tag already on this commit (an
# earlier run died before or during the upload), and the tag somewhere else
# (a version that was already released). Only the middle one is resumed.
TAG_AT_HEAD=0
if git -C "$ROOT" rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    if [ "$(git -C "$ROOT" rev-parse "$TAG^{commit}")" = "$(git -C "$ROOT" rev-parse HEAD)" ]; then
        TAG_AT_HEAD=1
        note "$TAG already names HEAD; resuming that release"
    else
        die "tag $TAG already exists, and not at HEAD; pick another version"
    fi
fi
if [ "$TAG_AT_HEAD" = 0 ] && git -C "$ROOT" ls-remote --exit-code --tags origin "$TAG" >/dev/null 2>&1; then
    die "tag $TAG already exists on origin; pick another version"
fi

RELEASE_EXISTS=0
if gh release view "$TAG" >/dev/null 2>&1; then
    [ "$TAG_AT_HEAD" = 1 ] || die "release $TAG already exists; delete it or pick another version"
    RELEASE_EXISTS=1
    note "release $TAG exists; its assets will be replaced"
fi

note "version : $CURRENT_VERSION -> $VERSION"
note "core    : $(cmake_version) -> $VERSION"
note "app     : $APP_NAME ($APP_ID)"
note "tag     : $TAG"
note "target  : $ARCH"
[ "$DRAFT" = 1 ]      && note "draft   : yes"
[ "$PRERELEASE" = 1 ] && note "pre     : yes"
[ "$DRY_RUN" = 1 ]    && note "dry run : build and package only, nothing published"

# --- version ---------------------------------------------------------------
step "Writing the version into package.json and cmake/Version.cmake"
if [ "$VERSION" = "$CURRENT_VERSION" ] && [ "$VERSION" = "$CURRENT_CMAKE_VERSION" ]; then
    note "already $VERSION in both files"
else
    if [ "$VERSION" = "$CURRENT_VERSION" ]; then
        note "package.json  : already $VERSION"
    else
        note "package.json  : $CURRENT_VERSION -> $VERSION"
    fi
    if [ "$VERSION" = "$CURRENT_CMAKE_VERSION" ]; then
        note "Version.cmake : already $VERSION"
    else
        note "Version.cmake : $CURRENT_CMAKE_VERSION -> $VERSION"
    fi
    if [ "$DRY_RUN" = 1 ]; then
        # electron-builder reads the version from package.json and puts it in
        # every file name, so a dry run has to write it too. Both files get put
        # back by the EXIT trap, because a dry run may not change the checkout.
        PKG_BACKUP="$(mktemp -t "$APP_SLUG-pkg-backup")"
        cp "$PKG" "$PKG_BACKUP"
        CMAKE_VERSION_BACKUP="$(mktemp -t "$APP_SLUG-cmake-version-backup")"
        cp "$CMAKE_VERSION_FILE" "$CMAKE_VERSION_BACKUP"
        note "[dry] both files restored at the end"
    fi
    write_version
fi

# --- build -----------------------------------------------------------------
if [ "$SKIP_BUILD" = 1 ]; then
    step "Build: skipped (--skip-build)"
    note "$OUT must already hold the artifacts for $VERSION"
else
    step "Building the core as WebAssembly"
    if [ "$SKIP_WASM" = 1 ]; then
        note "skipped (--skip-wasm); wasm/dist must already be current"
    else
        "$ROOT/wasm/build.sh"
    fi

    step "Building the front end"
    ( cd "$ELECTRON" && pnpm install --frozen-lockfile && pnpm run build )

    step "Packaging (.dmg and .zip)"
    ( cd "$ELECTRON" && pnpm exec electron-builder --mac dmg zip "--$ARCH" )
fi

# --- artifacts -------------------------------------------------------------
step "Collecting artifacts"

[ -d "$OUT" ] || die "no $OUT; a build should have made it"

ASSETS=()
while IFS= read -r f; do
    ASSETS+=("$f")
done < <(find "$OUT" -maxdepth 1 -name "$ARTIFACT_GLOB" \
            \( -name '*.dmg' -o -name '*.zip' -o -name '*.blockmap' \) | sort -u)
[ "${#ASSETS[@]}" -gt 0 ] || die "no artifacts for $VERSION in $OUT"

: > "$OUT/SHA256SUMS.txt"
for f in "${ASSETS[@]}"; do
    ( cd "$OUT" && shasum -a 256 "$(basename "$f")" ) >> "$OUT/SHA256SUMS.txt"
done
ASSETS+=("$OUT/SHA256SUMS.txt")

for f in ${ASSETS[@]+"${ASSETS[@]}"}; do
    printf '    %s  (%s)\n' "$(basename "$f")" "$(du -h "$f" | cut -f1)"
done

if [ "$DRY_RUN" = 1 ]; then
    step "Dry run: stopping before git"
    note "would commit the version, tag $TAG, push $BRANCH and the tag"
    note "would run: gh release create $TAG ... ${#ASSETS[@]} assets"
    exit 0
fi

# --- git -------------------------------------------------------------------
step "Committing and tagging"
if [ "$TAG_AT_HEAD" = 1 ]; then
    note "$TAG is already at HEAD; nothing to commit or tag"
else
    git -C "$ROOT" add "$PKG" "$CMAKE_VERSION_FILE"
    if git -C "$ROOT" diff --cached --quiet; then
        note "nothing to commit; $VERSION was already the version"
    else
        git -C "$ROOT" commit -m "chore: release $TAG"
    fi
    git -C "$ROOT" tag -a "$TAG" -m "$TITLE"

    note "pushing $BRANCH and $TAG"
    git -C "$ROOT" push origin "$BRANCH"
    git -C "$ROOT" push origin "$TAG"
fi

# --- notes -----------------------------------------------------------------
step "Writing the release notes"
NOTES="$(mktemp -t "$APP_SLUG-release-notes")"

if [ -n "$NOTES_FILE" ]; then
    [ -f "$NOTES_FILE" ] || die "no notes file at $NOTES_FILE"
    cat "$NOTES_FILE" > "$NOTES"
else
    PREV="$(git -C "$ROOT" describe --tags --abbrev=0 "$TAG^" 2>/dev/null || true)"
    {
        echo "## 安装"
        echo
        echo "1. 下载下面的 \`$DMG_NAME\`"
        echo "2. 把 **$APP_NAME** 拖进「应用程序」"
        echo "3. 首次打开若提示「已损坏，无法打开」，执行一次："
        echo
        echo '   ```bash'
        echo "   xattr -dr com.apple.quarantine \"/Applications/$APP_BUNDLE\""
        echo '   ```'
        echo
        # ${ARCH} with braces, not $ARCH: bash 3.2's parser swallows the
        # first byte of the full-width parenthesis and goes looking for a
        # variable called "ARCH\xef", which is the unbound variable it then
        # complains about.
        echo "Apple Silicon（${ARCH}），未做代码签名与公证。"
        echo
        echo "## 自上一个版本以来的变化"
        echo
        if [ -n "$PREV" ]; then
            echo "自 \`$PREV\` 以来："
            echo
            git -C "$ROOT" log --no-merges --pretty='- %s (%h)' "$PREV..$TAG"
        else
            echo "首个版本。"
            echo
            git -C "$ROOT" log --no-merges --pretty='- %s (%h)' -20 "$TAG"
        fi
        echo
        echo "## 校验"
        echo
        echo '```bash'
        echo 'shasum -a 256 -c SHA256SUMS.txt'
        echo '```'
        echo
        echo "仓库里不包含任何 ROM；请使用你合法拥有的游戏文件。"
    } > "$NOTES"
fi
note "$NOTES"

# --- upload ----------------------------------------------------------------
step "Creating the GitHub release"

GH_ARGS=(--title "$TITLE" --notes-file "$NOTES")
[ "$DRAFT" = 1 ]      && GH_ARGS+=(--draft)
[ "$PRERELEASE" = 1 ] && GH_ARGS+=(--prerelease)

if [ "$RELEASE_EXISTS" = 1 ]; then
    note "release $TAG exists; uploading assets over it"
    gh release upload "$TAG" "${ASSETS[@]}" --clobber
else
    gh release create "$TAG" "${GH_ARGS[@]}" "${ASSETS[@]}"
fi

URL="$(gh release view "$TAG" --json url -q .url)"
printf '\n\033[1m%s\033[0m\n' "Released: $URL"
if [ "$DRAFT" = 1 ]; then
    echo "It is a draft: publish it from that page when the dmg checks out."
fi
