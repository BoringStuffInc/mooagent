# List available recipes
default:
    @just --list

# Build the project
build:
    cargo build

# Build the project in release mode
build-release:
    cargo build --release

# Run the project
run *args:
    cargo run -- {{args}}

# Check code
check:
    cargo check

# Format code
fmt:
    cargo fmt

# Install the release version of the binary
install:
    cargo install --path . --locked
