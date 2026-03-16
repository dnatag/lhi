default := 'check'

# Quick compile check
check:
    cargo check

# Run all tests
test:
    cargo test

# Format code
fmt:
    cargo fmt

# Format check (CI-friendly)
fmt-check:
    cargo fmt -- --check

# Lint with clippy
clippy:
    cargo clippy -- -D warnings

# Debug build
build:
    cargo build

# Release build
build-release:
    cargo build --release

# Install debug binary to ~/bin
binstall:
    cargo build --bin lhi
    mv target/debug/lhi ~/bin/lhi

# Install release binary to ~/bin
binstall-release:
    cargo build --bin lhi --release
    mv target/release/lhi ~/bin/lhi

# Full CI pipeline: format check, lint, test
ci: fmt-check clippy test

# Dev workflow: format, lint, test
dev: fmt clippy test

# Clean build artifacts
clean:
    cargo clean

# Run lhi with arguments
run *args:
    cargo run --bin lhi -- {{args}}
