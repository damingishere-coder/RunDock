# alter-pm2 Justfile — common dev commands
# Install: cargo install just
# Usage:   just <recipe>

# @group Configuration : Default recipe — list all available commands
default:
    @just --list

# ─── Rust ──────────────────────────────────────────────────────────────────────

# @group BuildDev : Fast debug build
build:
    cargo build

# @group BuildDev : Release build with thin LTO (day-to-day release)
build-fast:
    cargo build --profile release-fast

# @group BuildDev : Full release build (CI / distribution)
release:
    cargo build --release --locked

# @group BuildDev : Watch + auto-rebuild on file changes (requires cargo-watch)
watch:
    cargo watch -x build

# @group BuildDev : Watch + auto-run the daemon on changes (requires cargo-watch)
watch-run:
    cargo watch -x run

# @group Testing : Run the same locked Rust test scope as CI
test-rust:
    cargo test --all-targets --all-features --locked

# @group Testing : Run cargo clippy with warnings-as-errors
lint-rust:
    cargo clippy --all-targets --all-features --locked -- -D warnings

# @group Testing : Audit dependencies for known CVEs (requires cargo-audit)
audit:
    cargo audit

# @group BuildDev : Format Rust code
fmt-rust:
    cargo fmt

# @group BuildDev : Build with tokio-console support (requires cargo-watch + unstable tokio)
#   Run `tokio-console` in a separate terminal to connect
console:
    cargo --config 'build.rustflags=["--cfg","tokio_unstable"]' build --features tokio-console
    cargo --config 'build.rustflags=["--cfg","tokio_unstable"]' run --features tokio-console -- daemon start

# ─── Frontend ─────────────────────────────────────────────────────────────────

# @group BuildUI : Start Vite dev server (proxies /api → daemon on :2999)
dev:
    cd web-ui && npm run dev

# @group BuildUI : Production frontend build
build-ui:
    cd web-ui && npm run build

# @group Testing : Run vitest (frontend unit tests)
test-ui:
    cd web-ui && npm run test

# @group Testing : Run vitest in watch mode
test-ui-watch:
    cd web-ui && npm run test:watch

# @group Testing : Run ESLint on frontend
lint-ui:
    cd web-ui && npm run lint

# @group BuildUI : Format frontend code with Prettier
fmt-ui:
    cd web-ui && npm run format

# ─── Combined ─────────────────────────────────────────────────────────────────

# @group Testing : Run all tests (Rust + frontend)
test: test-rust test-ui

# @group Testing : Lint everything
lint: lint-rust lint-ui

# @group BuildUI : Format everything
fmt: fmt-rust fmt-ui

# ─── Daemon helpers ───────────────────────────────────────────────────────────

# @group Utilities : Start the alter daemon
daemon-start:
    ./target/debug/alter daemon start

# @group Utilities : Stop the alter daemon
daemon-stop:
    ./target/debug/alter daemon stop

# @group Utilities : Show daemon status
daemon-status:
    ./target/debug/alter daemon status

# @group Utilities : Kill daemon so cargo build can replace the binary (Windows)
kill:
    ./target/debug/alter daemon stop

# ─── Install dev tools ────────────────────────────────────────────────────────

# @group Utilities : Install all recommended Rust dev tools
install-tools:
    cargo install cargo-watch
    cargo install cargo-nextest
    cargo install cargo-audit
    cargo install cargo-flamegraph
    cargo install tokio-console
