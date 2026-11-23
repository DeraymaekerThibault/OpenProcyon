#!/usr/bin/env bash
# Worktree lifecycle: warm new worktrees from the main checkout's build, remove them with their build, cap build disk use.
# usage: tools/worktree.sh new <branch> [<path>]   git worktree add (new branch off origin/main unless it exists; default path
#                                                 ../openprocyon-<branch>), then copies the main checkout's game/target/{debug,fast}
#                                                 (reflink: near free on btrfs/XFS) without incremental/, so only this crate builds
#        tools/worktree.sh rm <path>              deletes its game/target, then git worktree remove (refuses a dirty worktree)
#        tools/worktree.sh gc [-n]                every worktree: incremental/ older than GC_DAYS (7), local/{smokes,check,battle_check}
#                                                 files older than 14 days, `cargo clean --workspace` on a target over GC_CAP_GB (16; a warm worktree after --full is about 13);
#                                                 removed worktrees: their leftover target, then git worktree prune. -n: only print.
# WORKTREE_SEED=<target dir> copies another build than the main checkout's. Each worktree keeps its own target: do not share one
# CARGO_TARGET_DIR between active worktrees (one lock per profile, binaries overwrite each other); the scripts here unset it.
set -euo pipefail
unset CARGO_TARGET_DIR
main=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")
profiles="debug fast"
say() { echo "worktree.sh: $*"; }

new() {
    local branch=$1 wt=${2:-$main/../openprocyon-$1}
    if git -C "$main" show-ref -q --verify "refs/heads/$branch"; then git -C "$main" worktree add "$wt" "$branch"
    else git -C "$main" fetch -q origin main && git -C "$main" worktree add -b "$branch" "$wt" origin/main; fi
    wt=$(cd "$wt" && pwd)
    local seed=${WORKTREE_SEED:-$main/game/target} t=$SECONDS
    mkdir -p "$wt/game/target"
    for p in $profiles; do
        [ -d "$seed/$p" ] || { say "no $seed/$p to copy: the first $p build here is cold"; continue; }
        cp -a --reflink=auto "$seed/$p" "$wt/game/target/"
        rm -rf "$wt/game/target/$p/incremental"
        # A seed built before a profile or dependency change makes the first build here mostly cold.
        for f in Cargo.toml Cargo.lock; do
            if [ -f "$main/game/$f" ] && [ -z "$(find "$seed/$p/deps" -maxdepth 1 -newer "$main/game/$f" -print -quit 2>/dev/null)" ]; then
                say "warning: the $p build in $seed is older than game/$f: run tools/check.sh in the main checkout first"
            fi
        done
    done
    for f in Cargo.toml Cargo.lock; do
        [ ! -f "$main/game/$f" ] || cmp -s "$main/game/$f" "$wt/game/$f" ||
            say "warning: game/$f here differs from the main checkout's, whose build was copied (pull there and rebuild)"
    done
    say "copied the build in $((SECONDS - t))s: $(du -sh "$wt/game/target" | cut -f1) (logical size)"
    [ -e "$wt/.venv" ] || [ ! -d "$main/.venv" ] || ln -s "$main/.venv" "$wt/.venv"
    say "ready: $wt"
}

rm_wt() {
    local wt
    if [ ! -d "$1" ]; then # already deleted by hand: drop git's record of it
        wt=$(realpath -m "$1")
        git -C "$main" worktree list --porcelain | grep -qx "worktree $wt" || { say "$wt is not a worktree of $main"; exit 1; }
        git -C "$main" worktree prune
        say "removed $wt (its folder was already gone)"
        return
    fi
    wt=$(cd "$1" && pwd)
    [ "$wt" != "$main" ] || { say "refusing to remove the main checkout"; exit 1; }
    git -C "$main" worktree list --porcelain | grep -qx "worktree $wt" || { say "$wt is not a worktree of $main"; exit 1; }
    [ -z "$(git -C "$wt" status --porcelain)" ] || { say "$wt has uncommitted changes; commit or discard them first"; exit 1; }
    rm -rf "$wt/game/target"
    git -C "$main" worktree remove "$wt"
    say "removed $wt"
}

gc() {
    local dry= days=${GC_DAYS:-7} cap=${GC_CAP_GB:-16}
    [ "${1:-}" != -n ] || dry=1
    run() { echo "  $*"; [ -n "$dry" ] || "$@"; }
    local wt prunable
    while read -r key wt; do
        [ "$key" = worktree ] || continue
        prunable=$(git -C "$main" worktree list --porcelain | awk -v w="$wt" '$1=="worktree"{c=($2==w)} c&&$1=="prunable"{print 1}')
        if [ -n "$prunable" ]; then
            say "$wt: removed worktree"
            [ ! -d "$wt/game/target" ] || run rm -rf "$wt/game/target"
            continue
        fi
        local target=$wt/game/target
        if [ -d "$target" ]; then
            find "$target"/*/incremental -mindepth 1 -maxdepth 1 -mtime +"$days" 2>/dev/null | while read -r d; do run rm -rf "$d"; done || true
            local gb; gb=$(du -s --block-size=1G "$target" | cut -f1)
            if [ "$gb" -gt "$cap" ]; then
                say "$wt: target ${gb} GB > ${cap} GB"
                run cargo clean -q --manifest-path "$wt/game/Cargo.toml" --workspace
            fi
        fi
        for d in smokes check battle_check; do
            [ ! -d "$wt/local/$d" ] || find "$wt/local/$d" -type f -mtime +14 | while read -r f; do run rm -f "$f"; done
        done
        [ ! -d "$target" ] || say "$wt: $(du -sh "$target" | cut -f1)"
    done < <(git -C "$main" worktree list --porcelain)
    run git -C "$main" worktree prune
}

case "${1:-}" in
    new) shift; [ $# -ge 1 ] || { sed -n 3,4p "$0"; exit 2; }; new "$@" ;;
    rm) shift; [ $# -eq 1 ] || { sed -n 6p "$0"; exit 2; }; rm_wt "$1" ;;
    gc) shift; gc "$@" ;;
    *) sed -n 2,13p "$0"; exit 2 ;;
esac
