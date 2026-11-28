#!/usr/bin/env bash
# Writes a build provenance manifest for the current commit into $1 (a
# directory): the commit, branch, toolchain, lock file hash, the hashes of the
# artifacts found there, and whether the work tree was clean. Local only.
set -euo pipefail
out="${1:?usage: provenance.sh OUT_DIR}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
mkdir -p "$out"
{
  echo "# Build provenance"
  echo
  echo "- Commit: $(git rev-parse HEAD)"
  echo "- Branch: $(git rev-parse --abbrev-ref HEAD)"
  echo "- Commit date: $(git log -1 --format=%cI)"
  echo "- Work tree: $(if [ -z "$(git status --porcelain)" ]; then echo clean; else echo 'NOT CLEAN'; fi)"
  echo "- Upstream base: $(git merge-base HEAD main 2>/dev/null || echo unknown) (main)"
  echo "- Built: $(date -u +%FT%TZ) on $(uname -srm)"
  echo "- rustc: $(rustc --version)"
  echo "- cargo: $(cargo --version)"
  echo "- Cargo.lock sha256: $(shasum -a 256 Cargo.lock | cut -d' ' -f1)"
  echo "- Remotes: $(git remote | tr '\n' ' ')(none means nothing can be pushed from here)"
  echo
  echo "## Artifacts"
  for f in "$out"/*; do
    [ -f "$f" ] || continue
    case "$f" in *provenance*|*SHA256SUMS*) continue ;; esac
    echo "- $(basename "$f"): $(shasum -a 256 "$f" | cut -d' ' -f1)"
  done
  if [ -d "$out/Falcon Dev — Unified.app" ]; then
    echo "- Falcon Dev — Unified.app/Contents/MacOS/Falcon Dev: $(shasum -a 256 "$out/Falcon Dev — Unified.app/Contents/MacOS/Falcon Dev" | cut -d' ' -f1)"
    echo "- Bundle identifier: $(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$out/Falcon Dev — Unified.app/Contents/Info.plist" 2>/dev/null || echo unknown)"
  fi
} > "$out/BUILD-PROVENANCE.md"
echo "wrote $out/BUILD-PROVENANCE.md"
