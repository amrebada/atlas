  #!/usr/bin/env bash
  # bump-version.sh — usage: ./bump-version.sh <patch|minor|major|X.Y.Z>
  set -euo pipefail

  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  PKG="$ROOT/package.json"
  CARGO="$ROOT/src-tauri/Cargo.toml"
  TAURI="$ROOT/src-tauri/tauri.conf.json"

  for f in "$PKG" "$CARGO" "$TAURI"; do
    [[ -f "$f" ]] || { echo "missing: $f" >&2; exit 1; }
  done

  arg="${1:-}"
  [[ -z "$arg" ]] && { echo "usage: $0 <patch|minor|major|X.Y.Z>" >&2; exit 1; }

  current=$(sed -nE 's/.*"version":[[:space:]]*"([0-9]+\.[0-9]+\.[0-9]+)".*/\1/p' "$PKG" | head -n1)
  [[ -z "$current" ]] && { echo "could not read current version from $PKG" >&2; exit 1; }

  IFS='.' read -r MA MI PA <<< "$current"

  case "$arg" in
    major) new="$((MA+1)).0.0" ;;
    minor) new="${MA}.$((MI+1)).0" ;;
    patch) new="${MA}.${MI}.$((PA+1))" ;;
    *)
      if [[ "$arg" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        new="$arg"
      else
        echo "invalid bump: $arg" >&2; exit 1
      fi
      ;;
  esac

  echo "bumping $current -> $new"

  # macOS/BSD sed compatible (works on Linux too when -i '' is used)
  SED_INPLACE=(-i '')
  [[ "$(uname)" == "Linux" ]] && SED_INPLACE=(-i)

  # package.json: first "version": "..."
  sed "${SED_INPLACE[@]}" -E "0,/\"version\":[[:space:]]*\"[^\"]+\"/s//\"version\": \"$new\"/" "$PKG" 2>/dev/null \
    || sed "${SED_INPLACE[@]}" -E "s/\"version\":[[:space:]]*\"$current\"/\"version\": \"$new\"/" "$PKG"

  # tauri.conf.json: "version": "..."
  sed "${SED_INPLACE[@]}" -E "s/\"version\":[[:space:]]*\"$current\"/\"version\": \"$new\"/" "$TAURI"

  # Cargo.toml: top-level version = "..." (only first occurrence, under [package])
  awk -v new="$new" '
    BEGIN { done=0; in_pkg=0 }
    /^\[package\]/ { in_pkg=1; print; next }
    /^\[/ && !/^\[package\]/ { in_pkg=0; print; next }
    in_pkg && !done && /^version[[:space:]]*=/ { print "version = \"" new "\""; done=1; next }
    { print }
  ' "$CARGO" > "$CARGO.tmp" && mv "$CARGO.tmp" "$CARGO"

  # refresh Cargo.lock for the package entry (optional but recommended)
  if command -v cargo >/dev/null 2>&1; then
    (cd "$ROOT/src-tauri" && cargo update -p "$(sed -nE 's/^name[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' Cargo.toml | head -n1)"
  --precise "$new" 2>/dev/null || true)
  fi

  echo "done. updated:"
  echo "  $PKG"
  echo "  $TAURI"
  echo "  $CARGO"