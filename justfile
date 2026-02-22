# einked - local quality gates

default:
    @just --list

all: check-format lint check test doc
    @echo "All einked checks passed"

fmt:
    RUSTC_WRAPPER= cargo fmt --package einked --package einked-macros

check-format:
    RUSTC_WRAPPER= cargo fmt --package einked --package einked-macros -- --check

lint:
    RUSTC_WRAPPER= cargo clippy -p einked -p einked-macros --all-features -- -D warnings

check:
    RUSTC_WRAPPER= cargo check -p einked --all-features
    RUSTC_WRAPPER= cargo check -p einked-macros

test:
    RUSTC_WRAPPER= cargo test -p einked --all-features
    RUSTC_WRAPPER= cargo test -p einked-macros

doc:
    RUSTC_WRAPPER= RUSTDOCFLAGS='-D warnings' cargo doc -p einked -p einked-macros --all-features --no-deps
