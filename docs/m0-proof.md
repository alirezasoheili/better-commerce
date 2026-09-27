# M0 architecture proof

M0 proves that a fresh checkout can build and run the foundation example while
enforcing module-owned persistence, safe installation reconciliation, the
transport boundary, and ordered at-least-once outbox delivery. The required CI
status is **M0 architecture proof**; it succeeds only after contract governance,
format, workspace check/build/tests, Clippy, and the real Compose smoke all pass.
The aggregate runs after all gate outcomes and fails if any required gate failed,
was cancelled, or was skipped.

## Local verification

With Rust 1.98.1 and the `rustfmt` and `clippy` components from
[`rust-toolchain.toml`](../rust-toolchain.toml), run:

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
(cd contracts && buf lint && buf breaking proto --against baseline && buf generate)
git diff --exit-code -- modules/example-transport/src/generated
test -z "$(git ls-files --others --exclude-standard -- modules/example-transport/src/generated)"
python3 scripts/check_transport_boundary.py
python3 scripts/compose_smoke.py
```

The contract workflow pins Buf 1.73.0 and installs `protoc-gen-prost` 0.5.0
with `cargo install --locked`; generated Rust uses exact-pinned Prost 0.14.4.
CI checks both tracked and untracked generated output after generation. Workspace
Clippy lints are configured as warnings; CI passes `-D warnings`, so any Clippy
warning fails the gate.

The PostgreSQL integration tests require an isolated PostgreSQL server and
`BC_TEST_ADMIN_DATABASE_URL`; CI supplies PostgreSQL 18. Tests fail if that
variable is missing. The Compose proof requires Docker with the Compose plugin,
Python 3, the pinned Rust toolchain, and permission to pull the configured images.
It runs against a unique disposable Compose project and volume, then cleans them
up. See [`deploy/README.md`](../deploy/README.md) for local PostgreSQL and Compose
setup.

## Evidence and scope

PostgreSQL ownership and denial are proved by the real role and sibling-schema
tests in `core/tests/postgres.rs`. Transaction atomicity, acknowledgement and
retry, lost-ack duplicate tolerance, lease recovery and stale fencing, concurrent
dispatch, per-aggregate order, and dead-letter barriers are proved by the same
integration suite. `scripts/check_transport_boundary.py` checks crate/source
transport isolation; it is not the database ownership proof. Buf lint, breaking
baseline, generation, and generated-output checks run in the Protobuf contract
governance job. `scripts/compose_smoke.py` exercises first install, readiness,
repeat reconciliation, supported configuration change, initialized-module
removal rejection, and retained state.

The governing decisions are [ADR 0001](adr/0001-installation-isolation-and-reconciliation.md),
[ADR 0002](adr/0002-module-ownership-and-extraction-seam.md), and
[ADR 0003](adr/0003-module-owned-ordered-outbox.md). M0 remains a foundation
example: Catalog, Pricing, Inventory, Cart, Orders, Payments, and the purchase
path belong to M1.
