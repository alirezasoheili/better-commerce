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

M1 remains governed by ADR 0004 and its accepted specification/ticket graph. Runtime plugins remain outside M1.

---

## M2 — Runtime Plugin Platform

Goal:
Add a DNN-like installable extension platform without rebuilding Better Commerce or introducing per-plugin services.

Includes:

- Wasmtime-hosted WebAssembly Components;
- independently versioned WIT plugin contracts and official Rust SDK;
- local package install/upgrade/remove plus live enable/disable/config;
- plugin migrations, config/secrets and scoped files;
- plugin HTTP/UI bridge;
- PostgreSQL durable plugin jobs/schedules/event delivery;
- explicit commerce extension points including payment, pricing, shipping, checkout and structured storefront metadata/SEO;
- backup/restore integration, operator status, plugin CLI and developer hot reload/conformance tooling;
- realistic payment, dynamic pricing, SMS and UI/data reference plugins.

Exit condition:

All four reference plugins pass integrated restart, lost-response, compatibility, migration, job/event, UI and backup/restore acceptance while preserving M0/M1 module/outbox invariants. A deployment still requires only the BC executable, PostgreSQL and managed plugin package/data state; no Redis/message broker/plugin sidecars are required.

See `docs/specs/m2-runtime-plugin-platform.md` and `docs/plugin-platform/implementation-roadmap.md`.

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
Third-party plugin trust/marketplace only when product demand justifies it
