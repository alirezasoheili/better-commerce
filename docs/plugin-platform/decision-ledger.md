# Better Commerce V1 plugin platform — decision ledger

Status: approved architecture input for M2.

This ledger compresses the plugin-platform grilling session into durable rules. Detailed rationale and operational contracts live in `docs/specs/m2-runtime-plugin-platform.md` and ADR 0005.

## 1. Scope and trust

1. Better Commerce remains an opinionated commerce platform. Catalog/Products, Orders, Inventory, Customers, Payments, Shipping, Tax, Discounts and other canonical commerce capabilities remain core-owned.
2. V1 plugins are trusted first-party software. The V1 platform is not a hostile third-party marketplace sandbox and does not implement publisher signatures, permission approval, OAuth between host and plugin, or per-plugin capability policy.
3. WASM isolation is still used for ABI stability, memory/runtime fault containment and installability, not as a claim that malicious plugins are safely contained.

## 2. Runtime and ABI

4. Backend plugins are WebAssembly Components hosted in-process by Wasmtime. Installing a plugin never rebuilds Better Commerce and does not require a sidecar service, process, container or port.
5. WIT / the WebAssembly Component Model is the canonical plugin ABI source of truth. Protobuf remains available for non-plugin external contracts but does not duplicate plugin contracts.
6. WIT compatibility is package-versioned. Independently evolving contracts live in independently versioned WIT packages and are referenced as `namespace:package/interface@package-version`. V1 supports only the qualified interface references shipped by that BC release; there are no compatibility shims for older WIT package versions.
7. Rust is the officially supported V1 backend authoring language. The ABI stays language-neutral so future languages can target the same WIT contracts.
8. Each plugin has at most one `backend.wasm`. One long-lived instance is active per plugin and all invocations are serialized in V1. A timeout/trap discards the instance and recovery creates a new instance and reruns `initialize()`.
9. One global plugin resource policy covers execution timeout, Wasmtime compute/fuel/epoch policy and memory limits. Repeated traps/timeouts can open one simple per-plugin circuit breaker.

## 3. Extension model

10. Only BC core defines platform extension points. Plugins do not define platform-wide extension points or directly invoke another plugin by plugin ID.
11. Synchronous participation is allowed only through explicit BC-defined extension points. There are no generic global before/after hooks.
12. Each extension point declares its cardinality as either `single-provider` or `multi-contributor`, its deterministic ordering where relevant, and its own failure semantics.
13. Decision/veto behavior exists only where a specific extension contract defines it.
14. Storefront composition stays BC-owned. Plugins contribute structured data/decisions through explicit extension points rather than injecting arbitrary storefront HTML or routes.

## 4. Package and lifecycle

15. A plugin is one SemVer package with a stable immutable plugin ID and an immutable release descriptor. The package may contain `plugin.yaml`, optional `backend.wasm`, optional migrations, optional built UI, icons and read-only assets.
16. V1 install source is a local package. BC validates it, copies it into a BC-managed install directory, records plugin/version/descriptor digest and desired enabled state, then requires a BC restart for install/upgrade/remove package changes.
17. Live enable/disable and configuration changes do not require a restart. Permanent visible runtime states are intentionally small: `active`, `disabled`, `unavailable`.
18. Startup/enable activation is: package validation → compatibility validation → pending migrations → (when a backend exists) component instantiation → `initialize()` → health check → registration → `active`. Backend-less/UI-only plugins skip WASM lifecycle/health. Any failure makes only that plugin unavailable; BC still starts.
19. Normal uninstall preserves DB state and plugin runtime files. Destructive remove is explicit and first unloads the plugin.
20. Upgrade keeps config/secrets, applies immutable checksum-tracked migrations, and does not auto-rollback on failure. Downgrade is blocked by default with an explicit force escape hatch.

## 5. Data and transactions

21. PostgreSQL remains authoritative. BC owns one `sqlx::Pool`; WASM plugins use a thin host DB API rather than owning independent PostgreSQL pools/credentials.
22. Plugins own their tables/migrations and may create plugin-owned indexes, constraints, foreign keys to core tables, triggers on plugin-owned tables and SQL functions. They may not alter core-owned tables or add triggers to them.
23. Direct SQL reads of core tables and plugin-owned foreign keys to stable core IDs are allowed for trusted first-party V1 plugins and intentionally couple the plugin to the BC physical schema/compatibility range. ADR 0002's module-to-module extraction seam remains intact, but such plugins may need upgrade/migration/disable when referenced core storage moves. Core state mutation is not a supported arbitrary-SQL contract: canonical state changes use BC domain host APIs so invariants, snapshots, idempotency and events remain authoritative.
24. A plugin may start its own explicit DB transaction through the DB host API. It never implicitly joins a caller/core transaction.
25. Plugin business/filesystem state uses PostgreSQL for relational state and a plugin-scoped writable data directory for large/binary files. Packaged assets are read-only.

## 6. Jobs and events

26. Durable plugin jobs, event deliveries and schedules use one PostgreSQL-backed worker system. No Redis, NATS, RabbitMQ or separate broker is required.
27. Jobs/events are at-least-once. Stable IDs/idempotency keys are used where side effects can duplicate. Running jobs use leases/heartbeats so expired work is reclaimed after process failure.
28. Events remain post-commit. Core writes its transactional outbox first; the existing `EventDelivery` boundary acknowledges only after an idempotent PostgreSQL fan-out has durably created downstream plugin-delivery rows for matching subscriptions. Plugin workers later invoke WASM handlers from those rows. Core outbox publication therefore does not wait for plugin execution; plugin delivery remains at-least-once and ordered per plugin/aggregate while unrelated aggregates/plugins can advance independently.
29. Retries/backoff, dead-lettering, manual retry, optional TTL, queued cancellation, simple ordered job chains, static recurring schedules and optional progress/result metadata are V1 capabilities. No DAG/workflow engine is introduced.
30. Disabled plugins retain queued durable work; TTL continues to elapse. Removed subscriptions make obsolete old queued deliveries; incompatible old job types dead-letter rather than being silently remapped.

## 7. HTTP, UI and networking

31. Plugin UI is ordinary built browser assets, not WASM. BC serves it under `/extensions/{plugin-id}/*`; plugin-owned authenticated APIs live under `/extensions/{plugin-id}/api/*`; explicitly public/provider callbacks live under `/plugins/{plugin-id}/*`.
32. BC owns authentication and supplies a stable structured `PluginContext`; plugins do not couple to raw BC session/cookie internals.
33. Plugin pages can have one navigation entry under an Extensions area, own their internal subrouting and use a small frontend SDK for context, navigation, toast and theme.
34. Plugin-owned long-lived SSE handlers are not supported because V1 serializes one WASM instance. BC may provide generic host-owned SSE such as durable-job progress.
35. BC owns upload/download byte streaming. WASM receives scoped file handles/references instead of holding the instance open for large streams.
36. Full outbound HTTP/HTTPS, TCP, UDP, DNS and clocks/timers are enabled for trusted V1 through standard WASI capabilities. Autonomous infinite background loops inside WASM are unsupported; durable background work uses BC jobs/schedules.
37. Plugins cannot execute arbitrary OS processes and cannot read/write arbitrary host filesystem paths.

## 8. Configuration, errors and observability

38. Plugin config/secrets are stored by BC. Config schema is deliberately small (`string`, `secret`, `boolean`, `integer`, `number`, `enum` plus basic constraints/defaults); complex plugin configuration belongs in plugin tables/UI.
39. Secrets are encrypted at rest with a deployment key stored outside the database. Restoring encrypted config requires the same key; normal export redacts secrets and does not export plaintext secrets.
40. Candidate configuration is basic-schema validated and then plugin-validated before persistence. When an active runtime instance exists, BC calls `config_changed()` after save; otherwise the saved config is consumed by the next activation. Callback failure is reported without transactional rollback machinery.
41. Plugin errors use stable machine-readable structured errors and separate safe user messages from technical details. BC owns actual retry decisions.
42. BC enriches plugin logs/audit records with plugin ID/version and request/job/event/user context where available. V1 has no custom plugin metrics API; BC measures invocation health externally.

## 9. Commerce-specific authority

43. Pricing plugins return structured adjustments; BC calculates/persists final totals and requotes authoritatively at purchase submit. Changed totals require reconfirmation.
44. Payment plugins expose structured methods and provider operations. BC owns PurchaseAttempt/payment state, idempotency and canonical transitions; plugins own provider-specific calls, redirects and callback verification.
45. External provider operations that may have succeeded before a crash are treated as indeterminate, not blindly retried. BC supplies stable operation/reference identities and recovery/reconciliation paths.
46. Structured checkout fields, shipping/provider contracts, metadata/SEO, redirect, sitemap and robots contributions use explicit BC-defined contracts rather than arbitrary global mutation.

## 10. Backup, compatibility and completion

47. A BC backup is whole-install state: database, plugin tables/config metadata, managed package copies and plugin data directories. Backup is all-or-nothing and establishes a coherent snapshot boundary by pausing/draining plugin mutations as needed.
48. Restore restores exact captured plugin versions/state and does not run migrations during restore. Restore validates BC/plugin compatibility before mutation.
49. Plugins declare a BC SemVer compatibility range and exact WIT imports/exports; first-party plugins are continuously tested against current BC. Core schema direct-read coupling is protected by this compatibility/CI contract rather than a DB compatibility layer.
50. The V1 platform is not considered proven by loading `hello.wasm`. Acceptance requires realistic payment, dynamic pricing, SMS/event/job and UI/data reference plugins exercising the major seams.
