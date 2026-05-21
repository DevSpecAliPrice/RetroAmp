#!/usr/bin/env bash
# Rebuild the bundled skin seed library from the local RetroAmp database.
#
# Reads every skin marked is_favorite=1 in the developer's local catalog and
# copies the underlying .wsz files into <repo>/skins/, replacing whatever was
# there. The resulting directory is what ships with the installer (see
# tauri.conf.json -> bundle.resources) and what fresh installs receive via
# the seed-on-startup logic in src-tauri/src/skin/seed.rs.
#
# Run this after favouriting/unfavouriting skins in the running app when you
# want to update the seed set, then commit the resulting skins/ changes.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SKINS_DIR="$REPO_ROOT/skins"

DB="${RETROAMP_DB:-${XDG_CONFIG_HOME:-$HOME/.config}/retroamp/retroamp.db}"
if [ ! -f "$DB" ]; then
  echo "error: RetroAmp database not found at $DB" >&2
  echo "       run the app at least once, or set RETROAMP_DB to override." >&2
  exit 1
fi

mkdir -p "$SKINS_DIR"
find "$SKINS_DIR" -maxdepth 1 -name '*.wsz' -delete

copied=0
skipped=0
collide=0
while IFS= read -r src; do
  [ -z "$src" ] && continue
  if [ ! -f "$src" ]; then
    echo "warn: favourite path missing on disk: $src" >&2
    skipped=$((skipped + 1))
    continue
  fi
  case "$src" in
    *.wsz) ;;
    *)
      echo "warn: skipping non-.wsz favourite: $src" >&2
      skipped=$((skipped + 1))
      continue
      ;;
  esac
  dst="$SKINS_DIR/$(basename "$src")"
  if [ -e "$dst" ]; then
    echo "warn: name collision, keeping first copy: $(basename "$src")" >&2
    collide=$((collide + 1))
    continue
  fi
  cp "$src" "$dst"
  copied=$((copied + 1))
done < <(sqlite3 "$DB" "SELECT path FROM skin_catalog WHERE is_favorite=1 ORDER BY name;")

echo "rebuilt $SKINS_DIR — copied=$copied skipped=$skipped collisions=$collide"
du -sh "$SKINS_DIR"
