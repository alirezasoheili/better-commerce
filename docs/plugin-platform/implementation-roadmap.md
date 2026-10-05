# M2 runtime plugin platform — implementation roadmap

This is dependency order, not permission to start implementation. Convert milestones into repository tickets only after the architecture package is accepted/merged.

## P01 — Contract skeleton and package validator

Deliver:

- canonical WIT directory/layout and generation checks;
- plugin ID/SemVer/BC-range parsing;
- `plugin.yaml` parser + semantic validator;
- ZIP/path/package-size/digest validation;
- component import/export inspection, exact qualified WIT reference validation and descriptor/component semantic validation matrix;
- contract/breaking-change CI for WIT.

Exit proof: valid minimal component package passes; malformed/traversal/incompatible/WIT-drift packages fail deterministically.

## P02 — Wasmtime host and lifecycle kernel

Deliver:

- Wasmtime component engine/linker;
- one serialized instance per plugin;
- global timeout/memory/compute policy;
- instantiate/initialize/health/shutdown;
- trap/timeout recreation;
- simple circuit breaker;
- compiled-component cache.

Exit proof: lifecycle fixture survives trap/timeout recreation while BC remains healthy.

## P03 — Registry, managed packages and operator CLI

Deliver:

- plugin registry persistence;
- managed install/data directory layout;
- atomic local install, upgrade, enable, disable, normal remove, destructive remove;
- restart-required package lifecycle semantics;
- install attempt/error history;
- per-plugin lifecycle mutation lock;
- downgrade guard/force escape hatch.

Exit proof: install → restart activation, live disable/re-enable, failed upgrade/unavailable and preserved-data uninstall all match spec.

## P04 — Config, secrets, logging, context and Rust SDK

Deliver:

- host-context/config/log/audit WIT;
- config schema/default/basic constraints;
- candidate validation + live `config_changed`;
- encrypted secret storage using deployment key;
- generated Rust bindings and official SDK/error types;
- SDK fakes/mocks.

Exit proof: config/secret round-trip, redacted export, invalid-candidate rejection and SDK fixture tests.

## P05 — Database, migrations, files and WASI capabilities

Deliver:

- host DB API backed by shared `sqlx::Pool`;
- explicit plugin transaction handles;
- global plugin SQL statement timeout;
- plugin migration runner/history/checksums;
- plugin-owned schema restrictions/checks;
- scoped asset/data/temp file APIs;
- standard WASI HTTP/TCP/UDP/DNS/clocks;
- no process execution capability.

Exit proof: data plugin migration/transaction/restart tests, migration checksum rejection, scoped-file/path tests and outbound networking fixture.

## P06 — Plugin HTTP/UI bridge

Deliver:

- fixed `/extensions/{id}` and `/plugins/{id}` mounts;
- public/customer/merchant auth classification;
- stable PluginContext injection;
- plugin HTTP dispatch and stable errors;
- BC-owned upload/download streaming with scoped file refs;
- static UI serving, SPA fallback and immutable hashed assets;
- Extensions nav integration, frontend SDK and namespaced cookies;
- host-owned generic job/status SSE surface.

Exit proof: UI/API/public callback plugin works same-origin; large transfer does not hold WASM execution open.

## P07 — Durable jobs, schedules and event delivery

Deliver:

- PostgreSQL durable task tables/workers;
- leases/heartbeats/reclaim;
- retry/backoff/dead-letter/manual retry;
- TTL/cancel/progress/result/simple chains;
- static schedule reconciliation and missed-run-once behavior;
- existing transactional outbox → `EventDelivery` adapter that durably/idempotently fans out plugin-delivery rows before acknowledging core outbox acceptance;
- downstream WASM event worker with per-plugin/per-aggregate ordering, retry/dead-letter and independent aggregate/plugin progress;
- disabled-plugin queue retention and obsolete subscription handling.

Exit proof: crash/restart and duplicate-side-effect fixtures prove at-least-once/idempotency/order semantics.

## P08 — Extension-point registry and commerce contracts

Deliver stable WIT + host orchestration for all retained V1 extension families:

- payments;
- pricing;
- shipping;
- checkout structured fields;
- storefront metadata/SEO;
- redirects;
- sitemap;
- robots.

Also deliver cardinality (`single-provider` / `multi-contributor`), deterministic ordering/default provider selection and extension-specific failure semantics.

Exit proof: contract tests show no extension can bypass BC canonical pricing/payment state authority.

## P09 — Plugin CLI, status/admin and observability

Deliver:

- descriptor-declared plugin CLI commands/typed args;
- human/JSON structured output;
- minimal Extensions status page;
- last-error/status/config/enable-disable/open-plugin actions;
- BC external metrics for invocation health;
- request/job/event tracing enrichment and audit storage/rendering hooks.

Exit proof: operator can diagnose and retry common plugin failures without a full log/workflow UI.

## P10 — Backup and restore integration

Deliver:

- coherent plugin mutation quiescing/draining for backup snapshot;
- managed package/data-dir inclusion;
- encrypted-secret key-requirement metadata;
- all-or-nothing plugin backup participation;
- restore preflight compatibility;
- exact package/data restore with no migration during restore.

Exit proof: backup/restore of a stateful plugin reproduces the exact version/data and rejects incompatible target BC before mutation.

## P11 — Developer tooling and hot reload

Deliver:

- `better-commerce plugin dev`;
- dev backend component reload;
- frontend HMR/refresh integration;
- `better-commerce plugin test` conformance harness;
- example fixtures/reference package builder;
- dev debug info and production stripping/optimization.

Exit proof: a fresh plugin can be scaffolded/built/tested/reloaded without depending on BC internal Rust crates.

## P12 — Reference plugin: payment gateway

Exercise payment methods, public callback, provider signatures, stable provider operation IDs, duplicate callbacks, lost-response/indeterminate reconciliation and disable/upgrade paths using a local fake provider in CI.

## P13 — Reference plugin: dynamic/gold pricing

Exercise multi-contributor pricing, external HTTP, disposable memory cache, structured adjustments, deterministic IDs, requote/reconfirmation and required-contributor failure semantics.

## P14 — Reference plugin: SMS

Exercise post-commit event → durable job → external side effect, idempotency, retries, dead-letter, manual retry, TTL and disable/re-enable queue retention.

## P15 — Reference plugin: UI/data

Exercise plugin migrations/DB transactions, config/secrets, authenticated API, React/Vite UI, frontend SDK, file upload/export, job progress, CLI command, backup/restore and package upgrade.

## P16 — Integrated acceptance and architecture proof

Fresh installation proof covering:

- package install/upgrade/remove and live enable/disable;
- Wasmtime trap/timeout/circuit recovery;
- incompatible target-BC upgrade preflight;
- migration/config/secret/data/backup behavior;
- all four reference plugins together;
- M0/M1 regression invariants, especially module boundaries and transactional outbox semantics;
- no Redis/broker/plugin-sidecar/extra runtime dependency.
