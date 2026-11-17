#!/usr/bin/env bash
# Script: override_file_dates.sh
# Linux/BTRFS port of OverrideFileCreationDate.ps1
#
# Sets the "creation" (birth) time and the last-modified time of every file
# under a directory (recursively) to the current date and time.
#
# Why it isn't just `touch`:
#   * touch can set mtime/atime, but Linux has NO syscall to set a file's
#     birth time (btime / BTRFS "otime"). It is fixed when the inode is made.
#   * So, to get "created = now", each file is re-created as a NEW inode:
#       cp --reflink=always  -> on BTRFS this makes a new inode that SHARES the
#                               same data extents (copy-on-write): instant,
#                               no extra disk space, no data actually copied.
#       mv tmp -> original   -> atomic rename over the old file.
#   * ctime (inode change time) is always "now" after any change anyway.
#
# Side effects of re-creating the inode (things NTFS SetCreationTime avoids):
#   * The inode number changes; hard links to the file are split apart.
#   * Programs holding the file open keep the old (now unlinked) version.
#   Mode, owner and xattrs (incl. ACLs, SELinux labels) are preserved.
#
# Usage: override_file_dates.sh [-v] [-n] [-m] [PATH]
#   -v  verbose: print every file (the .ps1 had $Verbose, but it was never set)
#   -n  no reflink fallback: fail instead of doing a full copy on non-CoW FS
#   -m  mtime only: just `touch`, don't re-create inodes (birth time untouched)

set -uo pipefail

# Predefined path (equivalent of C:\Users\prophet\Pictures\Ultimate_Deck)
TARGET="$HOME/Pictures/Ultimate_Deck"

VERBOSE=0
REFLINK_MODE="auto"   # auto = reflink if possible, else a real copy
MTIME_ONLY=0

while getopts ":vnm" opt; do
    case "$opt" in
        v) VERBOSE=1 ;;
        n) REFLINK_MODE="always" ;;
        m) MTIME_ONLY=1 ;;
        *) echo "Usage: $0 [-v] [-n] [-m] [PATH]" >&2; exit 2 ;;
    esac
done
shift $((OPTIND - 1))
[[ $# -ge 1 ]] && TARGET="$1"

GREEN=$'\e[32m'; YELLOW=$'\e[33m'; CYAN=$'\e[36m'; RED=$'\e[31m'; RESET=$'\e[0m'

# Check if path exists
if [[ ! -d "$TARGET" ]]; then
    echo "${RED}Error: Path '$TARGET' does not exist.${RESET}" >&2
    exit 1
fi

modified=0
failed=0

refresh_file() {
    local f="$1" dir base tmp
    dir=$(dirname -- "$f")
    base=$(basename -- "$f")
    tmp=$(mktemp -p "$dir" ".${base}.XXXXXX") || return 1

    # New inode => new birth time. --preserve copies mode/owner/xattrs but
    # deliberately NOT timestamps.
    if ! cp --reflink="$REFLINK_MODE" --preserve=mode,ownership,xattr \
            -- "$f" "$tmp" 2>/dev/null; then
        # ownership can fail for files owned by another user; retry without it
        cp --reflink="$REFLINK_MODE" --preserve=mode,xattr -- "$f" "$tmp" \
            || { rm -f -- "$tmp"; return 1; }
    fi
    mv -f -- "$tmp" "$f" || { rm -f -- "$tmp"; return 1; }
}

# -print0 / read -d '' handles spaces, newlines etc. in file names
while IFS= read -r -d '' file; do
    now=$(date '+%Y-%m-%d %H:%M:%S.%N')   # per-file "Get-Date", like the .ps1
    ok=1
    if [[ $MTIME_ONLY -eq 0 ]]; then
        refresh_file "$file" || ok=0
    fi
    # Set both access and modification time to "now"
    if [[ $ok -eq 1 ]] && touch -d "$now" -- "$file"; then
        modified=$((modified + 1))
        if [[ $VERBOSE -eq 1 ]]; then
            echo "${GREEN}✅ File '$file' creation and last modified times updated to: ${now%.*}${RESET}"
            # stat %w = birth time ("-" if the FS/kernel can't report it)
            stat -c '   birth: %w | modify: %y' -- "$file"
        fi
    else
        failed=$((failed + 1))
        echo "${YELLOW}⚠️ Failed to update creation or last modified time of '$file'${RESET}" >&2
    fi
done < <(find "$TARGET" -type f -print0)

echo "${CYAN}🎉 Done! Updated creation and last modified time for $modified files.${RESET}"
[[ $failed -gt 0 ]] && echo "${YELLOW}($failed files failed)${RESET}"
exit 0
