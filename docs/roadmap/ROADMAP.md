# Better Commerce Roadmap

## M0 — Foundation

Goal:
Establish the architecture and tooling required to safely build Better Commerce.

Includes:

- architecture decisions required before implementation
- Rust workspace skeleton
- contracts repository/layout
- kernel walking skeleton
- module interface convention
- transactional outbox foundation
- public HTTP API convention
- testing conventions
- CI baseline

Exit condition:

A minimal server boots, connects to PostgreSQL, exposes `/healthz`, and
composes one deliberately trivial example/foundation module at startup from
the resolved installation configuration. The module owns its PostgreSQL
schema migration, which the reconciler invokes before restart. Running code
proves an application-facing Rust port with a local/in-process adapter, one
state change that atomically writes
a durable outbox event, and at-least-once outbox dispatch in an integration
test. CI runs contract lint and breaking checks, practical module-boundary
checks, and workspace build, test, and lint checks.

M0 establishes the seam for later standalone module extraction; it does not
implement a remote gRPC adapter, dynamic module activation, or the M1 purchase
path.

---

## M1 — Commerce Tracer Bullet

One complete purchase path:

Product
→ Price
→ Inventory
→ Cart
→ Order
→ Fake Payment

Plus the thinnest possible storefront/admin surfaces required to prove the architecture.

---

## Later

Catalog V1
Inventory V1
Checkout V1
Payments V1
Shipping V1
Promotions V1
Search V1
Customers V1
Operational tooling
