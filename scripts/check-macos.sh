#!/bin/sh
# Type-checks the workspace for macOS from another OS, so a change made on Linux
# can't quietly break the Mac build. `cargo check` never links, so C dependencies
# only need their build scripts to succeed: a stub compiler and archiver stand in
# for Apple's toolchain. This proves the Rust compiles, not that it runs; CI on
# macos-latest still runs the tests.
set -eu

target=aarch64-apple-darwin
root=$(cd "$(dirname "$0")/.." && pwd)
stub="$root/target/macos-check-stub"
mkdir -p "$stub"

cat > "$stub/cc" <<'STUB'
#!/bin/sh
while [ $# -gt 0 ]; do
    case "$1" in
        -o) shift; : > "$1" ;;
        --version) echo "stub 0.0.0" ;;
    esac
    shift
done
STUB
cat > "$stub/ar" <<'STUB'
#!/bin/sh
while [ $# -gt 0 ]; do
    case "$1" in *.a) : > "$1" ;; esac
    shift
done
STUB
chmod +x "$stub/cc" "$stub/ar"

rustup target add "$target" >/dev/null 2>&1 || true
export CC_aarch64_apple_darwin="$stub/cc" CXX_aarch64_apple_darwin="$stub/cc" AR_aarch64_apple_darwin="$stub/ar"
exec cargo check --workspace --all-targets --target "$target" "$@"
