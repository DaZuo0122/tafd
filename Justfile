# TAFD Justfile
# Requires: cargo, just

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
    @New-Item -ItemType Directory -Force -Path package/tafd | Out-Null
    @Copy-Item -Path target/release/tafd.exe -Destination package/tafd/ -Force
    @Copy-Item -Path assets -Destination package/tafd/ -Recurse -Force
    @Write-Host "Package ready in package/tafd/"

# Clean build artifacts
[confirm]
clean:
    cargo clean
    if (Test-Path package) { Remove-Item -Recurse -Force package }
