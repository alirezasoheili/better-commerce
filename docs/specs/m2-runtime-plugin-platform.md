# M2 — Runtime plugin platform specification

Status: accepted architecture for M2.

## 1. Goal

M2 gives Better Commerce a DNN-like extension installation experience without rebuilding the BC executable:

```text
better-commerce plugin install ./zarinpal.zip
# restart BC
# plugin activates if compatible, configured and healthy
```

A normal deployment remains:

```text
Better Commerce executable
+ PostgreSQL
+ managed plugin packages/data
```

No sidecar plugin service, per-plugin container, message broker, Node runtime, extra port or inter-process authentication is required.

M2 is post-M1. It does not alter the accepted M1 commerce tracer-bullet scope or ticket graph.

## 2. Non-goals

V1 does not provide:

- hostile/untrusted third-party plugin security guarantees;
- a marketplace, publisher-signature or package-provenance system;
- OAuth/JWT between BC and in-process plugins;
- per-plugin permission approval or fine-grained capability policy;
- multiple simultaneous versions of one plugin ID;
- arbitrary storefront DOM/HTML injection or arbitrary global route claiming;
- generic before/after hooks around core operations;
- plugin-defined platform extension points;
- a generic plugin entity/CRUD/custom-field framework; plugin-specific extra data uses plugin-owned side tables;
- direct plugin-to-plugin calls by plugin ID;
- a distributed workflow engine, DAG engine or external broker;
- plugin-owned long-lived SSE/WebSocket execution;
- arbitrary subprocess/shell execution;
- multiple backend WASM components per plugin.

## 3. Core architecture

### 3.1 Runtime

BC embeds Wasmtime and loads WebAssembly Components from managed plugin packages.

For each active plugin:

- there is one long-lived component instance in V1;
- all invocations for that plugin are serialized across HTTP, extension points, jobs, events and CLI commands;
- different plugins may execute independently;
- one global host policy bounds memory/compute/execution time;
- a timeout or trap aborts the invocation, discards the instance and recreates it;
- recreated instances rerun `initialize()` before becoming callable;
- repeated timeout/trap failures may open a simple host-defined circuit breaker;
- synchronous calls fail fast while the circuit is open;
- durable jobs/events remain queued/retryable while the circuit is open.

Persistent correctness-critical state belongs in PostgreSQL or plugin storage, never only in WASM memory.

### 3.2 ABI

WIT / WebAssembly Component Model contracts are the sole plugin ABI source of truth. Compatibility is versioned at the WIT package level. Contracts that need independent compatibility evolution live in independently versioned packages rather than sharing one giant package. Exact interface references use the qualified `namespace:package/interface@package-version` form. A BC release advertises the exact interface references it implements; V1 does not load compatibility adapters for older package versions.

The compiled component's imports/exports must match `plugin.yaml`. Descriptor/component drift is rejected before activation. Semantic declarations are validated too: declaring routes requires the plugin HTTP export; jobs require the jobs export; event subscriptions require the events export; CLI commands require the CLI export; each commerce extension declaration requires its corresponding extension export; and any backend component must export lifecycle. Backend-less/UI-only packages declare none of those backend-only capabilities.

Rust is the supported V1 backend language. Generated Rust bindings plus an official SDK are the normal authoring surface. Plugins do not depend on `better-commerce-core` Rust crates directly.

### 3.3 Trust model

Plugins are trusted first-party software in V1. BC therefore intentionally avoids marketplace-style permission/security ceremony. The host still gives plugins structured APIs and scoped file handles because stable contracts and fault containment are valuable even for trusted code.

Standard WASI outbound HTTP/HTTPS, TCP, UDP, DNS and clocks/timers are enabled globally for active V1 plugins. Arbitrary host process execution is not exposed.

## 4. Extension-point model

Only BC core defines extension points. Each contract declares:

- WIT package/interface/version;
- cardinality: `single-provider` or `multi-contributor`;
- deterministic contributor order where applicable;
- invocation checkpoint(s);
- timeout/failure meaning;
- whether failure blocks, degrades or falls back;
- DTOs containing only the minimum required stable data.

No generic global hook can veto a core operation. A plugin can veto only where the specific extension contract defines a synchronous decision.

Initial V1 extension families include the architecture needed for:

- payment providers and structured payment methods;
- pricing adjustments at explicit quote checkpoints;
- shipping-rate/provider behavior;
- structured checkout fields;
- structured storefront metadata/SEO, redirects, sitemap and robots contributions;
- event consumers;
- plugin-owned HTTP routes and CLI commands.

Additional extension points may be added only as BC-defined WIT contracts, not by reusing internal Rust APIs.

## 5. Package model

Canonical package shape:

```text
plugin.zip
├── plugin.yaml
├── backend.wasm          # optional
├── migrations/           # optional
│   ├── 0001_init.sql
│   └── ...
├── ui/                   # optional built static frontend
│   ├── index.html
│   └── assets/...
└── assets/               # optional read-only runtime assets/icons/docs
```

Rules:

- plugin ID is globally stable, immutable, URL-safe and reused for routes, registry keys and data directories;
- version is SemVer;
- descriptor is immutable per release;
- one installed version per plugin ID;
- one optional `backend.wasm` per package;
- backend and UI are independently optional;
- package extraction rejects path traversal and unsafe paths;
- one globally configurable package-size limit applies;
- BC records plugin ID, version and descriptor digest;
- package assets are locally bundled; plugin UI does not depend on arbitrary external CDNs.

Managed install layout is deterministic, for example:

```text
<bc-data>/plugins/installed/{plugin-id}/{version}/...
<bc-data>/plugins/data/{plugin-id}/...
```

BC does not retain old package versions after a successful replacement; backups capture exact installed package versions.

## 6. Lifecycle

### 6.1 Persistent visible states

Keep the operator-visible model intentionally small:

- `active`
- `disabled`
- `unavailable`

Registry metadata additionally records installed version, descriptor digest, desired enabled state, last seen/activation time and last error.

### 6.2 Install

`better-commerce plugin install <local-package>`:

1. parse and structurally validate package;
2. validate ID/version/BC compatibility/WIT declarations/config schema/migration layout/UI paths;
3. reject duplicate/conflicting plugin identity;
4. copy package atomically into BC-managed storage;
5. register desired state as enabled;
6. record install attempt/result;
7. require restart.

The CLI does not run migrations or hot-load the component. Startup owns the one canonical activation path.

### 6.3 Startup/enable activation

For an enabled plugin:

1. validate managed package/digest;
2. validate BC version, required BC capabilities and WIT imports/exports;
3. verify required configuration exists;
4. run pending plugin migrations transactionally, one migration at a time;
5. if `backend.wasm` exists, instantiate component;
6. if a backend exists, call `initialize()`;
7. if a backend exists, call health check;
8. if backend health is `healthy`/`degraded`, or if the package is backend-less, register declared routes/extensions/events/jobs/schedules/UI navigation that are valid for the package shape;
9. mark `active` (with degraded health separately observable if needed).

Any failure makes only that plugin `unavailable`; BC keeps starting. `unhealthy` does not activate. Initialize failure does not use the runtime circuit breaker because activation never completed.

Disabled plugins are skipped completely: no migrations, instance, health, routes, jobs, schedules or event delivery.

### 6.4 Disable / re-enable

Disable is live:

- reject new plugin invocations;
- let the current serialized invocation finish up to global timeout;
- invoke best-effort short-deadline `shutdown()`;
- discard instance;
- preserve package/config/DB/data/migrations and queued durable work.

Re-enable runs the full normal activation path including pending migrations.

### 6.5 Upgrade

Upgrade validates/copies/registers the new package and requires restart. Existing config/secrets are preserved. New required config with no value makes the plugin unavailable/config-incomplete after restart.

Startup then runs new pending migrations and normal activation. If migration succeeds but component load/init later fails, applied migrations remain and the new plugin stays installed but unavailable. There is no automatic package/database rollback. A corrected compatible release is installed explicitly.

Applied migrations store version/name and checksum; modifying an already-applied migration is rejected. Downgrade is blocked by default; `--force` is an explicit operator escape hatch.

### 6.6 Remove

Normal remove disables/unloads and removes package/UI registration while preserving plugin tables, migration history, config and data directory.

Destructive remove is explicit, first unloads the plugin and drains active work, then deletes retained plugin data according to the removal operation.

## 7. Configuration and secrets

Descriptor-generated config supports a deliberately small schema:

- `string`
- `secret`
- `boolean`
- `integer`
- `number`
- `enum`

Basic validation may include required/default, min/max, min/max length and pattern. Lists/nested generic schemas are not part of descriptor config V1; complex configuration belongs in plugin-owned tables/UI.

Config is deployment-global in V1, not per-merchant/store override inside one installation.

Save flow:

```text
candidate values
→ descriptor validation
→ backend exists?
   ├─ no  → descriptor validation is sufficient
   └─ yes → call validate_config(candidate) on the active instance or, when no active instance exists, on a fresh transient validation instance
→ persist config/secrets
→ if an active runtime instance exists, call config_changed()
```

`validate_config(candidate)` is explicitly callable before `initialize()` on a fresh transient instance and must receive the candidate values directly; validation therefore does not require the plugin to already be active or the candidate to already be stored. The transient validation instance is discarded after validation. Validation failure rejects the save. If no active runtime exists, a valid save is persisted and consumed by the next activation. A later `config_changed()` failure on an active plugin is reported and can make behavior degraded/unavailable as appropriate, but V1 does not build distributed rollback of the already-saved config.

Secrets are encrypted at rest with a deployment-level key supplied outside the database. Normal config export redacts secrets. Plaintext secret export is not a normal supported operation. Restore of encrypted secrets requires the corresponding deployment key.

## 8. Data and migration contract

BC owns one PostgreSQL pool. Plugins call a WIT DB host API backed by that pool; they do not receive PostgreSQL credentials and do not create independent pools.

Plugin DB API supports query/execute plus explicit begin/commit/rollback for plugin-owned transactions. A plugin invocation never implicitly joins the core caller's transaction.

### 8.1 Plugin-owned schema

Plugin migrations may create/manage plugin-owned:

- tables;
- indexes;
- constraints;
- foreign keys, including references to stable core IDs;
- triggers only on plugin-owned tables;
- SQL functions.

Installing PostgreSQL extensions requires explicit operator/BC configuration.

Plugin migrations may not alter BC-owned core tables or add triggers to core tables. Default/seed data is applied through ordinary versioned plugin migrations; V1 has no separate seed/install hook.

### 8.2 Core data access

Trusted V1 plugins may directly read core tables. Plugin-owned tables may also reference stable core IDs with foreign keys. This is intentional physical-schema coupling and is covered by plugin BC compatibility ranges and coordinated CI. ADR 0002's module-to-module extraction seam remains intact, but a plugin that takes this direct-read/FK option is not transparently insulated from a future core schema move or module extraction; such a change may require the plugin to be upgraded, migrated or disabled.

Canonical core state mutation is performed through BC domain host APIs wherever it can affect invariants, state machines, events, snapshots or idempotency. Arbitrary direct SQL updates to core business state are not the supported plugin contract. Holding a plugin DB transaction open across external network calls is strongly discouraged by SDK/docs but is not technically prohibited in trusted V1.

Host/domain APIs expose explicit stable WIT DTOs rather than serializing internal Rust structures. Potentially large collections are paginated.

### 8.3 Files

- package assets are read-only and plugin-scoped;
- each plugin has a writable data directory;
- APIs expose scoped handles/relative paths, never arbitrary host paths;
- large/binary output belongs in plugin files or DB/blob storage, not job result payloads;
- disk-full and ordinary I/O failures are structured plugin I/O failures; V1 has no per-plugin disk quota framework.

## 9. Durable jobs, schedules and events

BC provides one PostgreSQL-backed durable execution subsystem for plugin jobs, scheduled work and event delivery.

### 9.1 Jobs

Capabilities:

- one-off durable enqueue;
- optional idempotency key;
- globally defined retry/backoff/max-attempt policy;
- lease/heartbeat recovery of abandoned running work;
- dead-letter/failed terminal state;
- manual retry;
- cancellation while queued;
- optional `expires_at` TTL;
- simple ordered job chains;
- optional 0–100 progress and short message;
- small structured result payload/reference;
- completed-history retention/pruning;
- no job-priority system: eligible work is ordered by scheduled/queued time subject to leases and per-aggregate event ordering.

Effective V1 execution concurrency remains one invocation at a time per plugin because the plugin has one serialized component instance.

A BC crash after an external side effect but before job completion persistence yields at-least-once re-execution. Plugin handlers must use stable job/operation IDs for idempotent side effects.

### 9.2 Schedules

Recurring schedules are statically declared in `plugin.yaml`. Runtime creation of arbitrary recurring schedules is deferred. Missed schedules run once after recovery rather than replaying every missed tick. On upgrade BC reconciles future schedules to the new descriptor; already-enqueued executions retain their identity.

Autonomous infinite/background loops inside WASM are unsupported. Long-running/repeating work uses durable jobs/schedules.

### 9.3 Events

Core business events remain owned by core modules and use their transactional outboxes. Plugin handling is always post-commit; synchronous business decisions belong to extension points.

The ADR 0003 handoff boundary is preserved explicitly:

```text
core module transaction + module outbox commit
→ shared outbox dispatcher
→ plugin EventDelivery adapter
→ transactionally fan out durable plugin-delivery rows for matching active subscriptions
→ commit plugin-delivery rows
→ acknowledge acceptance to the core outbox

later:
plugin delivery worker
→ direct WASM event handler invocation
→ success / retry / dead-letter
```

The fan-out is idempotent, with a uniqueness key equivalent to `(source_event_id, plugin_id, event_type, event_version)`. The core outbox is therefore not held unpublished by a slow, disabled or broken plugin after the durable downstream handoff has succeeded.

Plugin-delivery semantics:

- worker → WASM call is direct, not HTTP-to-self;
- at-least-once;
- original stable source event ID is preserved;
- plugin delivery is marked complete only after handler success;
- plugin can classify a failure as non-retryable, causing immediate dead-letter;
- ordering is preserved per plugin and aggregate; a dead-lettered delivery blocks later deliveries for that same plugin/aggregate until explicit operator retry/skip resolution;
- failure for one plugin/aggregate does not block unrelated aggregates or other plugins;
- original event ID is preserved on manual replay;
- disabled plugins retain already-fanned-out deliveries, while TTL continues to elapse;
- removing a subscription on upgrade makes queued deliveries for that removed subscription obsolete/cancelled;
- plugins do not publish canonical BC events and V1 has no plugin-to-plugin event bus.

## 10. HTTP and UI

### 10.1 Routes

Namespaces are fixed:

- `/extensions/{plugin-id}/*` — plugin UI, with SPA fallback except API paths;
- `/extensions/{plugin-id}/api/*` — authenticated plugin API dispatch;
- `/plugins/{plugin-id}/*` — explicitly declared public/provider callback routes.

A route descriptor classifies authorization as `public`, `customer` or `merchant`. BC does not enforce versioning of a plugin's internal route API; a plugin may version its own namespace if desired. BC resolves auth and supplies `PluginContext`; plugins do not access raw internal session implementations.

`PluginContext` contains stable request/invocation facts such as request ID, locale/currency/deployment context and optional authenticated merchant/customer IDs. Public routes receive reduced context with no invented identity.

Plugins can read request headers and set headers/status codes on their own responses, subject to BC overriding host-critical security headers. CORS remains globally BC-owned. Public plugin routes use one global BC rate-limit policy. BC also applies one global ordinary request-body limit and a separate larger global upload limit; V1 has no per-plugin body-size policy.

Duplicate provider callbacks are expected and must be idempotent.

### 10.2 UI

UI is built static HTML/CSS/JS/TypeScript (typically React/Vite), shipped in the package and served same-origin. Dependencies are bundled locally.

The small BC frontend SDK provides only stable host integration such as:

- verified context;
- navigation;
- toast/notifications;
- theme.

Plugins own internal routing. A UI plugin may declare one Extensions-area navigation entry. Backend-only plugins do not get a nav entry by default but remain visible on the Extensions status page.

Hashed assets receive immutable caching; `index.html` is short/no-cache. Plugin UI shares one BC-controlled CSP baseline, bundles dependencies locally, does not use arbitrary CDNs, and should call its backend rather than provider APIs directly from the browser. Plugin service workers are unsupported. Internal plugin UI URL compatibility is plugin-owned.

If backend is unavailable, static UI may still load and receives plugin status; API calls return stable `plugin_unavailable` errors.

### 10.3 Upload/download and realtime

BC streams incoming uploads to plugin-scoped temp/data storage and passes a file reference to WASM. For download/export, WASM returns a scoped file/reference plus metadata, ends the invocation, and BC streams bytes to the client.

Plugin-owned long-lived SSE/WebSocket handlers are not supported in V1 because one serialized instance would block the plugin. BC may expose generic host-owned SSE for job/status progress.

### 10.4 Cookies

Plugin cookies use reserved namespacing such as `bc_plugin_{plugin_id}_*` to avoid BC-session collisions. Plugin controls its own cookie attributes within its route namespace; BC reserves security-critical/core cookie names.

## 11. Commerce extension contracts

### 11.1 Pricing

Pricing contributors return structured adjustments (`id`, kind, amount, label and contract-specific metadata). BC namespaces adjustment IDs by plugin ID, calculates/persists authoritative totals and owns arithmetic.

Pricing runs only at explicit checkpoints such as product estimate, cart quote, checkout authoritative quote and purchase-submit final validation. Purchase submit always requotes. If the customer-visible total changed, BC requires reconfirmation rather than silently charging a new amount.

Contributor failure semantics are defined by the extension contract; required price-affecting contributors can block an authoritative purchase calculation, while optional informational contributors may be omitted.

### 11.2 Payments

Payment plugins expose one or more structured payment methods and provider operations. BC owns purchase/payment state, idempotency and canonical payment transitions. Plugin owns provider-specific transaction creation, redirects, callbacks/signature verification and mapping provider states into canonical BC states.

Every provider-side operation that could be duplicated receives a stable BC operation/reference identity. If the provider may have accepted an operation before the plugin/BC crashed, BC records the result as indeterminate and recovery/reconciliation queries/resolves it; it is never converted to a definitive decline merely because time elapsed and is never blindly repeated when duplication is unsafe.

### 11.3 Storefront metadata and SEO

Plugins can contribute structured title, description, canonical, robots, OpenGraph, Twitter/X card data, JSON-LD, alternates/hreflang, redirects, sitemap entries and robots additions through explicit contracts. No arbitrary raw `<head>` HTML/script injection is provided.

Plugins cannot mutate arbitrary global response headers through metadata contracts; only explicitly supported structured header contributions may be added.

For singleton metadata, BC defaults apply first and active contributors run in configured deterministic order, with later contributions winning. JSON-LD need only be valid JSON; BC does not attempt to understand every schema vocabulary.

## 12. Observability and operator surfaces

### 12.1 Logging and tracing

Plugins use a structured logging host API. BC enriches records with plugin ID/version and request/job/event/user context where available.

Every HTTP/plugin invocation receives a BC request ID. Background jobs inherit the originating request ID when available. V1 does not introduce a separate general correlation-ID framework.

BC measures invocation count, duration, success/failure, timeout, trap, circuit state and job/event failures externally; there is no custom plugin metrics API V1.

### 12.2 Audit

Plugins emit audit entries through a BC host audit API rather than writing audit tables directly. BC enriches entries with plugin/version/context/timestamp.

### 12.3 Extensions page

A deliberately small page displays installed plugin name/icon/version/status/last error, configuration link, enable/disable and open-plugin action when UI exists. It is not a marketplace, log browser, migration browser or package installer.

## 13. Package/route/job errors

Plugin-facing errors are stable structured values with at least:

- `code`;
- safe `user_message` where a user-facing message is appropriate;
- `technical_message` for logs/operator surfaces;
- `retryable` hint.

BC owns actual retry policy and maps plugin/runtime failures to domain-appropriate storefront messages; raw Wasmtime/provider/internal errors never appear to customers.

## 14. Backup and restore

BC backup covers the entire installation state relevant to plugins:

- core + plugin database state/config metadata;
- encrypted secrets as stored;
- managed exact plugin packages;
- plugin writable data directories;
- plugin registry metadata.

Backup is all-or-nothing. To obtain a coherent DB + file snapshot, BC pauses new plugin mutations and drains/coordinates active plugin file/database work as required. Half-written plugin files are not copied.

The deployment encryption key is not silently bundled into ordinary backup material; backup metadata indicates the key requirement.

Restore validates that the selected BC binary is compatible with the captured plugin set before mutating the installation, restores the exact snapshot/package versions, and does not run plugin migrations during restore. Upgrading after restore is a separate action.

## 15. Compatibility

Each package declares:

- plugin SemVer;
- BC SemVer compatibility range;
- required BC capabilities/modules;
- exact WIT imports/exports.

A plugin may declare support for multiple BC major versions if it genuinely passes their contracts. Incompatible plugins block a BC upgrade before core state is mutated unless those plugins are disabled/upgraded first.

First-party plugin CI runs against current supported BC versions. Direct core-table reads intentionally rely on this compatibility discipline rather than a separate database compatibility facade.

## 16. Developer experience

BC ships:

- canonical WIT files;
- generated Rust bindings;
- an official `better-commerce-plugin-sdk` with its own SemVer;
- host API fakes/mocks for unit tests;
- `better-commerce plugin dev` and `plugin test` tooling;
- production package/component validation;
- dev-mode backend WASM reload and frontend HMR/refresh support;
- compiled Wasmtime component cache keyed by component digest + runtime version;
- dev builds with useful debug/source information and optimized/stripped production packages.

Plugin-specific CLI commands remain a V1 capability. `plugin.yaml` declares simple typed arguments, BC parses/validates them and plugin returns structured output that BC can render human-readable or JSON.

## 17. Acceptance reference plugins

M2 is complete only when the host is proven with four realistic plugins and fresh-runtime failure testing.

1. **Payment gateway plugin** — single payment provider, methods, provider transaction creation, redirect/callback verification, duplicate callback handling, indeterminate/lost-response recovery.
2. **Dynamic/gold pricing plugin** — multi-contributor pricing, outbound HTTP, in-memory disposable cache, deterministic adjustment IDs, required-contributor failure behavior.
3. **SMS plugin** — post-commit event subscription, durable enqueue, external provider side effect, retry/idempotency/dead-letter/manual retry, disable/re-enable retention.
4. **UI/data plugin** — plugin migrations/tables, authenticated API, React/Vite UI, config, file upload/export, job progress and upgrade behavior.

CI/integration fixtures may use local fake provider servers; the acceptance value is exercising real host boundaries rather than depending on production third parties.

## 18. Definition of done

The V1 platform is done when:

- package install/upgrade/remove and live enable/disable work with restart semantics above;
- incompatible/broken plugins never prevent BC startup, while incompatible BC upgrades are rejected before mutation;
- WIT/descriptor/component compatibility is validated;
- migrations/checksums/config/secrets/data directories work under upgrade, uninstall, destructive remove, backup and restore;
- serialized Wasmtime execution, timeout/trap recreation and circuit behavior are proven;
- core mutations retain BC invariants/outbox authority;
- PostgreSQL-backed jobs/events/schedules survive restart and prove at-least-once/idempotency/ordering/dead-letter semantics;
- UI/API/public-callback namespaces and stable auth context work same-origin;
- large file traffic is host-streamed without holding WASM invocations open;
- all retained V1 extension families are represented by WIT contracts and contract tests;
- official SDK/dev/test/hot-reload workflow is usable;
- the four reference plugins pass integrated acceptance, including crash/lost-response/restart tests.
