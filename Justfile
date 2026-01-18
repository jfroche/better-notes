default:
    @just --list

# Run the standup command
standup *args="":
    cargo run -- standup {{ args }}

# Run tests
test:
    cargo nextest run

# Run clippy
lint:
    cargo clippy --all-targets -- --deny warnings

# Format code
fmt:
    cargo fmt

# Check formatting
fmt-check:
    cargo fmt -- --check

# Build release
build:
    cargo build --release

# Run with tracing
trace *args="":
    RUST_LOG=better_notes=trace cargo run -- {{ args }}
