# Better Commerce plugin platform architecture package

Authoritative M2 architecture inputs:

- [ADR 0005](../adr/0005-m2-runtime-plugin-platform.md) — architectural decision and relationship to M0/M1.
- [M2 specification](../specs/m2-runtime-plugin-platform.md) — complete V1 behavioral contract.
- [Decision ledger](decision-ledger.md) — compact durable rules distilled from the grilling session.
- [WIT architecture](wit-architecture.md) — package/interface ownership and ABI rules.
- [`plugin.yaml` contract](plugin-manifest.md) — descriptor semantics and package declarations.
- [Lifecycle](lifecycle.md) — activation, runtime recovery, disable/upgrade/remove and crash behavior.
- [Implementation roadmap](implementation-roadmap.md) — dependency-ordered P01–P16 delivery plan.
- [Reference plugin acceptance](reference-plugin-acceptance.md) — payment, dynamic pricing, SMS and UI/data proof matrix.

The package is intentionally post-M1. It does not alter the accepted M1 tracer-bullet ticket graph.
