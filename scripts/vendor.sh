#!/usr/bin/env bash
# Maintains the patched crates in this repository.
#
# Each top-level folder holds one crate: `crate/` is the crates.io release
# with the folder's `patches/*.patch` applied in order. A folder with an
# UPSTREAM file takes its crate from a Git commit instead (see upstream_*). This script checks
# that this still holds, rebuilds `crate/` (to upgrade to a new release), and
# records edits made in `crate/` as a new patch.
#
#   scripts/vendor.sh add <crate> <ver>       start a folder from a release
#   scripts/vendor.sh check [crate...]        crate/ equals its release + patches
#   scripts/vendor.sh rebuild <crate> [ver]   rebuild crate/ from release ver
#                                             (default: the current one; a
#                                             Git crate uses its UPSTREAM)
#   scripts/vendor.sh save <crate> <name> [description...]
#                                             record crate/'s edits since the
#                                             last patch as patches/NNNN-<name>.patch
#
# Needs bash, curl, tar, git and shasum or sha256sum.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/picacho-patches"

die() { echo "error: $*" >&2; exit 1; }

all_crates() {
    local dir
    for dir in "$ROOT"/*/crate/Cargo.toml; do
        [ -e "$dir" ] || continue
        basename "$(dirname "$(dirname "$dir")")"
    done
}

crate_dir() {
    [ -f "$ROOT/$1/crate/Cargo.toml" ] || die "no crate folder '$1'"
    echo "$ROOT/$1"
}

# The [package] version of a crate's Cargo.toml.
current_version() {
    awk '
        /^\[/ { in_pkg = ($0 == "[package]") }
        in_pkg && /^version *=/ { gsub(/.*= *"|".*/, ""); print; exit }
    ' "$1/crate/Cargo.toml"
}

sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

# The checksum crates.io's index records for name@version.
index_checksum() {
    local name lower path
    name="$1"
    lower="$(echo "$name" | tr '[:upper:]' '[:lower:]')"
    case ${#lower} in
        1) path="1/$lower" ;;
        2) path="2/$lower" ;;
        3) path="3/${lower:0:1}/$lower" ;;
        *) path="${lower:0:2}/${lower:2:2}/$lower" ;;
    esac
    curl -sSfL "https://index.crates.io/$path" \
        | grep -F "\"vers\":\"$2\"" \
        | sed -n 's/.*"cksum":"\([0-9a-f]*\)".*/\1/p' \
        | head -n 1
}

# Downloads name@version from crates.io (once), verifying its checksum, and
# prints the path of the .crate file.
fetch() {
    local name="$1" version="$2" file expected
    file="$CACHE/$name-$version.crate"
    if [ ! -f "$file" ]; then
        mkdir -p "$CACHE"
        expected="$(index_checksum "$name" "$version")"
        [ -n "$expected" ] || die "$name $version is not on crates.io"
        curl -sSfL "https://static.crates.io/crates/$name/$name-$version.crate" -o "$file.part"
        [ "$(sha256 "$file.part")" = "$expected" ] || { rm -f "$file.part"; die "checksum mismatch for $name $version"; }
        mv "$file.part" "$file"
    fi
    echo "$file"
}

# A value from a folder's UPSTREAM file (`key=value` lines; `#` comments).
upstream_value() {
    sed -n "s/^$2=//p" "$ROOT/$1/UPSTREAM" | head -n 1
}

# Downloads the UPSTREAM commit's listed paths (once) and prints the path
# of their tar archive. UPSTREAM names `url`, `commit` (a full hash) and
# `paths` (space-separated, relative to the repository root).
fetch_upstream() {
    local name="$1" url commit paths file work
    url="$(upstream_value "$name" url)"
    commit="$(upstream_value "$name" commit)"
    paths="$(upstream_value "$name" paths)"
    [ -n "$url" ] && [ -n "$commit" ] && [ -n "$paths" ] || die "$name/UPSTREAM needs url, commit and paths"
    echo "$commit" | grep -Eq '^[0-9a-f]{40}$' || die "$name/UPSTREAM: commit must be a full hash"
    file="$CACHE/$name-$commit.tar"
    if [ ! -f "$file" ]; then
        mkdir -p "$CACHE"
        work="$(mktemp -d)"
        git -C "$work" init -q
        git -C "$work" fetch -q --depth 1 "$url" "$commit" || { rm -rf "$work"; die "cannot fetch $commit from $url"; }
        # shellcheck disable=SC2086 # paths is a list
        git -C "$work" archive --format=tar -o "$file.part" "$commit" -- $paths
        rm -rf "$work"
        mv "$file.part" "$file"
    fi
    echo "$file"
}

# Extracts name@version (or its UPSTREAM commit) into dest and applies the
# folder's patches. With keep_rejects set, a patch that doesn't apply
# leaves .rej files.
build() {
    local name="$1" version="$2" dest="$3" keep_rejects="${4:-}" file patch
    mkdir -p "$dest"
    if [ -f "$ROOT/$name/UPSTREAM" ]; then
        file="$(fetch_upstream "$name")"
        tar -xf "$file" -C "$dest"
    else
        file="$(fetch "$name" "$version")"
        tar -xzf "$file" -C "$dest" --strip-components 1
    fi
    for patch in "$ROOT/$name"/patches/*.patch; do
        [ -e "$patch" ] || continue
        if ! (cd "$dest" && git apply --whitespace=nowarn "$patch" 2>/dev/null); then
            if [ -n "$keep_rejects" ]; then
                (cd "$dest" && git apply --whitespace=nowarn --reject "$patch") || true
                die "$(basename "$patch") does not apply to $name $version; see the .rej files in $dest"
            fi
            die "$(basename "$patch") does not apply to $name $version"
        fi
    done
}

cmd_add() {
    [ $# -eq 2 ] || die "usage: vendor.sh add <crate> <version>"
    local name="$1" version="$2"
    [ ! -e "$ROOT/$name" ] || die "$name already exists"
    mkdir -p "$ROOT/$name/patches"
    build "$name" "$version" "$ROOT/$name/crate"
    echo "added $name $version; add its README.md and license files"
}

patch_count() {
    local patch count=0
    for patch in "$1"/patches/*.patch; do
        [ -e "$patch" ] && count=$((count + 1))
    done
    echo "$count"
}

cmd_check() {
    local crates name dir work failed=0
    if [ $# -gt 0 ]; then crates="$*"; else crates="$(all_crates)"; fi
    for name in $crates; do
        dir="$(crate_dir "$name")"
        work="$(mktemp -d)"
        build "$name" "$(current_version "$dir")" "$work/crate"
        if diff -rq "$work/crate" "$dir/crate" >/dev/null; then
            echo "ok      $name $(current_version "$dir")"
        else
            echo "DIFFERS $name: crate/ is not the release plus patches/:"
            diff -rq "$work/crate" "$dir/crate" | sed "s|$work/crate|release+patches|; s|$dir/crate|crate|" || true
            failed=1
        fi
        rm -rf "$work"
    done
    return "$failed"
}

cmd_rebuild() {
    [ $# -ge 1 ] || die "usage: vendor.sh rebuild <crate> [version]"
    local name="$1" dir version work
    dir="$(crate_dir "$name")"
    version="${2:-$(current_version "$dir")}"
    work="$(mktemp -d)"
    build "$name" "$version" "$work/crate" keep
    rm -rf "$dir/crate"
    mv "$work/crate" "$dir/crate"
    rm -rf "$work"
    echo "rebuilt $name $version from crates.io and $(patch_count "$dir") patches"
}

cmd_save() {
    [ $# -ge 2 ] || die "usage: vendor.sh save <crate> <name> [description...]"
    local name="$1" patch_name="$2" dir work count out status
    shift 2
    dir="$(crate_dir "$name")"
    work="$(mktemp -d)"
    build "$name" "$(current_version "$dir")" "$work/a"
    cp -R "$dir/crate" "$work/b"
    mkdir -p "$dir/patches"
    count="$(patch_count "$dir")"
    out="$dir/patches/$(printf '%04d' $((count + 1)))-$patch_name.patch"
    {
        [ $# -gt 0 ] && printf '%s\n\n' "$*"
        status=0
        (cd "$work" && git -c core.quotepath=off diff --no-index --no-color --no-ext-diff \
            --binary --src-prefix= --dst-prefix= a b) || status=$?
        [ "$status" -le 1 ] || die "git diff failed"
    } >"$out"
    rm -rf "$work"
    if ! grep -q '^diff --git' "$out"; then
        rm -f "$out"
        die "crate/ has no edits beyond the existing patches"
    fi
    echo "wrote ${out#"$ROOT"/}"
}

case "${1:-}" in
    add) shift; cmd_add "$@" ;;
    check) shift; cmd_check "$@" ;;
    rebuild) shift; cmd_rebuild "$@" ;;
    save) shift; cmd_save "$@" ;;
    *) sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
