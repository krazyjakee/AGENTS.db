#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Install the `agentsdb` CLI locally.

Usage:
  bash scripts/install-local.sh [options]

Options:
  --cargo-install        Use `cargo install --path ...` (default).
  --prefix PATH          Build + copy into PATH (installs to PATH/bin by default).
  --bin-dir PATH         Install directory for the binary (overrides --prefix/bin).
  --features FEATURES    Cargo features to enable (default: all-embedders).
  --debug                Build debug binary (default: release).
  --force                Overwrite existing binary, if present.
  -h, --help             Show this help.

Examples:
  bash scripts/install-local.sh
  bash scripts/install-local.sh --prefix "$HOME/.local"
  bash scripts/install-local.sh --bin-dir /usr/local/bin --force
EOF
}

want_cargo_install=1
prefix=""
bin_dir=""
profile="release"
force=0
features="all-embedders"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --cargo-install)
      want_cargo_install=1
      shift
      ;;
    --prefix)
      want_cargo_install=0
      prefix="${2:-}"
      shift 2
      ;;
    --bin-dir)
      want_cargo_install=0
      bin_dir="${2:-}"
      shift 2
      ;;
    --features)
      features="${2:-}"
      shift 2
      ;;
    --debug)
      profile="debug"
      shift
      ;;
    --force)
      force=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      echo >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found; install Rust first: https://rustup.rs/" >&2
  exit 1
fi

script_dir="$(
  cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1
  pwd -P
)"

repo_root=""
if command -v git >/dev/null 2>&1; then
  if repo_root="$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null)"; then
    :
  else
    repo_root=""
  fi
fi
if [[ -z "$repo_root" ]]; then
  repo_root="$(cd -- "$script_dir/.." && pwd -P)"
fi

crate_path="$repo_root/crates/agentsdb-cli"
if [[ ! -d "$crate_path" ]]; then
  echo "Could not find crate at: $crate_path" >&2
  exit 1
fi

# Build frontend
frontend_path="$repo_root/crates/agentsdb-web/frontend"
if [[ -d "$frontend_path" ]]; then
  if ! command -v npm >/dev/null 2>&1; then
    echo "npm not found; install Node.js first: https://nodejs.org/" >&2
    exit 1
  fi

  echo "+ building frontend at $frontend_path"
  (
    cd "$frontend_path"
    npm install
    npm run build
  )
else
  echo "Warning: frontend directory not found at: $frontend_path" >&2
fi

if [[ "$want_cargo_install" -eq 1 ]]; then
  args=(install --path "$crate_path" --locked)
  if [[ -n "$features" ]]; then
    args+=(--features "$features")
  fi
  if [[ "$force" -eq 1 ]]; then
    args+=(--force)
  fi
  echo "+ cargo ${args[*]}"
  cargo "${args[@]}"
  echo "Installed: agentsdb"
  exit 0
fi

if [[ -z "$bin_dir" ]]; then
  if [[ -z "$prefix" ]]; then
    prefix="$HOME/.local"
  fi
  bin_dir="$prefix/bin"
fi

target_flag="--release"
target_dir="$repo_root/target/release"
if [[ "$profile" == "debug" ]]; then
  target_flag=""
  target_dir="$repo_root/target/debug"
fi

features_flag=""
if [[ -n "$features" ]]; then
  features_flag=(--features "$features")
fi

echo "+ cargo build -p agentsdb-cli ${target_flag} --locked ${features_flag[*]}"
cargo build -p agentsdb-cli ${target_flag} --locked "${features_flag[@]}"

uname_s="$(uname -s 2>/dev/null || true)"
exe_suffix=""
case "$uname_s" in
  MINGW*|MSYS*|CYGWIN*)
    exe_suffix=".exe"
    ;;
esac

src_bin="$target_dir/agentsdb${exe_suffix}"
if [[ ! -f "$src_bin" ]]; then
  echo "Build succeeded but binary not found at: $src_bin" >&2
  exit 1
fi

mkdir -p "$bin_dir"
dst_bin="$bin_dir/agentsdb${exe_suffix}"

if [[ -e "$dst_bin" && "$force" -ne 1 ]]; then
  echo "Already exists: $dst_bin" >&2
  echo "Re-run with --force to overwrite." >&2
  exit 1
fi

echo "+ install $src_bin -> $dst_bin"
if command -v install >/dev/null 2>&1; then
  # On macOS/Linux, `install` handles perms; on Git Bash it may not exist.
  install -m 0755 "$src_bin" "$dst_bin"
else
  cp -f "$src_bin" "$dst_bin"
  chmod +x "$dst_bin" 2>/dev/null || true
fi

echo "Installed: $dst_bin"
