# WIT architecture

WIT is the canonical plugin ABI. Do not mirror these contracts in Protobuf or expose internal Rust types.

## Naming/version rules

- Namespace: `better-commerce:*`.
- Every package/interface is independently SemVer-versioned.
- Stable interfaces start at `1.0.0` when committed as supported V1 contracts.
- Experimental interfaces use an explicitly experimental package name and `0.x` versions, for example `better-commerce:experimental-search@0.1.0`; they carry no compatibility promise.
- BC supports the exact interface versions shipped with that BC release in V1; no old-version adapters are required.
- `plugin.yaml` declares exact imports/exports and package validation compares them with component metadata.

## Host imports available to plugins

| Package | Interface purpose |
| --- | --- |
| `better-commerce:host-context@1.0.0` | Stable invocation/request identity and locale/currency/deployment context. |
| `better-commerce:host-config@1.0.0` | Read config/secrets made available by BC. Candidate-config validation receives candidate values through lifecycle export rather than mutating stored config. |
| `better-commerce:host-db@1.0.0` | Query/execute, pagination-friendly row values, explicit plugin transactions, statement timeout; backed by BC `sqlx::Pool`. |
| `better-commerce:host-files@1.0.0` | Read-only package assets, scoped writable plugin data/temp file handles, metadata; no arbitrary host paths. |
| `better-commerce:host-jobs@1.0.0` | Enqueue durable jobs/chains, optional idempotency/TTL, progress/result reporting for current job where applicable. |
| `better-commerce:host-log@1.0.0` | Structured logging; BC adds plugin/invocation metadata. |
| `better-commerce:host-audit@1.0.0` | Structured audit entry emission. |
| `better-commerce:catalog@1.0.0` | Published stable catalog reads/mutations required by plugins; core invariants remain in Catalog. |
| `better-commerce:pricing-core@1.0.0` | Published pricing reads/operations that are not the pricing-contributor callback itself. |
| `better-commerce:inventory@1.0.0` | Published inventory operations required by plugins; no direct arbitrary mutation contract. |
| `better-commerce:orders@1.0.0` | Published order/purchase operations safe for plugin use. |
| `better-commerce:customers@1.0.0` | Published customer operations when that context exists. |

Core-domain host packages are introduced only when a real plugin needs the operation. Direct core SQL reads remain allowed for trusted V1, but domain APIs are preferred and are mandatory for canonical core state mutation.

Outbound networking and clocks use standard WASI packages/capabilities rather than BC-specific socket/HTTP wrappers.

## Common plugin exports

| Package | Interface purpose |
| --- | --- |
| `better-commerce:plugin-lifecycle@1.0.0` | `initialize`, best-effort `shutdown`, `health`, `validate-config`, `config-changed`. |
| `better-commerce:plugin-http@1.0.0` | Dispatch plugin-owned API/public routes using stable request/response DTOs and scoped body/file references. |
| `better-commerce:plugin-events@1.0.0` | Handle post-commit BC events with stable event ID and retry classification. |
| `better-commerce:plugin-jobs@1.0.0` | Execute named durable job handlers; report structured success/failure/result. |
| `better-commerce:plugin-cli@1.0.0` | Execute descriptor-declared plugin CLI commands with parsed typed args and structured output. |

A plugin exports only the interfaces it actually implements. Backend-less/UI-only plugins export none.

## Commerce extension exports

| Package | Cardinality | Responsibility |
| --- | --- | --- |
| `better-commerce:payments@1.0.0` | single-provider per configured payment provider/method selection | Structured methods, create/resolve provider operation, callback verification/mapping. BC owns canonical payment/purchase state. |
| `better-commerce:pricing@1.0.0` | multi-contributor | Structured adjustments at explicit BC checkpoints. BC owns final arithmetic/persistence/requote. |
| `better-commerce:shipping@1.0.0` | single-provider or multi-contributor as contract sub-interface defines | Structured rates/eligibility/provider operations. |
| `better-commerce:checkout@1.0.0` | multi-contributor | Structured checkout fields/validation; no arbitrary checkout HTML injection. |
| `better-commerce:storefront-metadata@1.0.0` | multi-contributor | Title/meta/canonical/robots/OG/Twitter/JSON-LD/alternate contributions. |
| `better-commerce:storefront-routing@1.0.0` | multi-contributor with deterministic conflict handling | Structured redirects only; no arbitrary core storefront route ownership. |
| `better-commerce:sitemap@1.0.0` | multi-contributor | Structured sitemap entries. |
| `better-commerce:robots@1.0.0` | multi-contributor | Structured robots additions; BC renders final file. |

Do not collapse these into a giant `plugin-api-v1` interface. Independent versioning limits blast radius.

## DTO rules

- Explicit WIT records/enums only; never serialize internal Rust structs as the public contract.
- Include only the minimum data needed for the extension operation.
- IDs are stable opaque strings/UUID-shaped values as defined by the owning core context; plugins must not infer database internals from an ID.
- Money uses explicit currency + checked integer minor units, matching core semantics.
- Potentially large collections use cursor/page contracts.
- Errors are explicit records with stable machine code, retryable hint and safe/technical message separation where applicable.

## Invocation context

`host-context` exposes the current stable context rather than raw Axum/session objects. Conceptual fields:

```text
request_id
plugin_id
plugin_version
locale
currency
merchant_id?     # when authenticated/meaningful
customer_id?     # when authenticated/meaningful
job_id?          # background invocation
origin_event_id? # event-triggered invocation
origin_request_id?
```

Public routes receive only context that actually exists; BC never invents customer/merchant identity.

## HTTP body model

Plugin HTTP export receives metadata plus either a small bounded inline body or a BC-owned scoped body/file handle. Large uploads/downloads are streamed by BC and represented to WASM by handles/references. Plugin responses can return ordinary small bodies or a file reference for BC to stream.

## Lifecycle pseudocontract

Conceptually:

```text
initialize() -> result
shutdown() -> result
health() -> { healthy | degraded | unhealthy, message? }
validate_config(candidate-config) -> validation-result
config_changed() -> result
```

`initialize` may use config/DB/logging/network but must be bounded by the global invocation timeout and must not start autonomous infinite background work.
