# TAFD Justfile
# Requires: cargo, just, sh

set shell := ["sh", "-c"]

bin := if os() == "windows" { "target/release/tafd.exe" } else { "target/release/tafd" }

# Default recipe: list available recipes
default:
    @just --list

# Run the daemon in development mode
run:
    cargo run --bin tafd -- --verbose

# Build release binary
build:
    cargo build --release

# Create a release package directory (binary + assets)
package: build
    mkdir -p package/tafd
    cp {{bin}} package/tafd/
    cp -r assets package/tafd/
    echo "Package ready in package/tafd/"

# Clean build artifacts
[confirm]
clean:
    cargo clean
    rm -rf package
