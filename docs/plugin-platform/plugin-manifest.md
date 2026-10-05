# `plugin.yaml` V1 contract

`plugin.yaml` is author-controlled, immutable for a published plugin version and validated before BC copies a package into managed storage.

The exact serialization parser can evolve during implementation, but the following semantic fields are the V1 contract.

## Example

```yaml
schema_version: 1

id: zarinpal
name: Zarinpal
version: 1.2.0
summary: Zarinpal payment provider for Better Commerce

author:
  name: Better Commerce
  homepage: https://example.invalid/better-commerce
support:
  homepage: https://example.invalid/support

requires:
  better_commerce: ">=0.2.0 <0.3.0"
  capabilities:
    - orders
    - pricing

package:
  backend: backend.wasm
  ui: ui/
  icon: assets/icon.svg

wit:
  imports:
    - better-commerce:host-context/context@1.0.0
    - better-commerce:host-config/config@1.0.0
    - better-commerce:host-db/database@1.0.0
    - better-commerce:host-log/logging@1.0.0
    - better-commerce:orders/orders@1.0.0
  exports:
    - better-commerce:plugin-lifecycle/lifecycle@1.0.0
    - better-commerce:plugin-http/handler@1.0.0
    - better-commerce:plugin-events/consumer@1.0.0
    - better-commerce:plugin-jobs/handler@1.0.0
    - better-commerce:plugin-cli/command@1.0.0
    - better-commerce:payments/provider@1.0.0

extensions:
  payments:
    - id: zarinpal
      display_name: Zarinpal

routes:
  - mount: extension-api
    path: /*
    auth: merchant
  - mount: public
    path: /callback/*
    auth: public

navigation:
  label: Zarinpal
  path: /

config:
  merchant_id:
    type: string
    required: true
  sandbox:
    type: boolean
    default: true
  api_key:
    type: secret
    required: true
  region:
    type: enum
    values: [ir]
    default: ir

jobs:
  definitions:
    - name: reconcile-payment
      args:
        payment_operation_id:
          type: string
          required: true
    - name: reconciliation-sweep
  schedules:
    - id: reconciliation-sweep
      job: reconciliation-sweep
      cron: "0 * * * *"

cli:
  commands:
    - name: test-connection
      args:
        verbose:
          type: boolean
          default: false

events:
  subscriptions:
    - type: order.paid
      version: 1
```

## Identity

- `id` is lowercase URL-safe and globally stable for the life of the plugin.
- `version` is SemVer.
- BC rejects a package that reuses the same plugin ID/version with a different descriptor/component digest.
- Only one version of a plugin ID can be installed at a time.

## Metadata

`name`, `summary`, author/publisher, support/homepage and local icon are informational/operator UI metadata. They do not establish cryptographic trust in V1.

## Compatibility

`requires.better_commerce` is a SemVer range. `requires.capabilities` lists BC capabilities/modules that must exist for activation.

`wit.imports` and `wit.exports` list exact qualified WIT interface references in `namespace:package/interface@package-version` form. BC validates them against component metadata and the host's supported interface set before instantiation. WIT compatibility versions packages; independently evolving BC contracts therefore use separate packages.

## Package declarations

Backend and UI are independently optional. Package paths are relative and must remain inside the package. Extraction rejects absolute paths, traversal and unsafe duplicates.

## Extension declarations

Descriptor extension declarations identify which BC-defined extension contracts should be registered and any stable provider/contributor IDs needed by that contract. The descriptor cannot invent a new platform extension point.

Where an extension point supports configured ordering/default selection, BC configuration/registry owns that selection; the plugin does not globally self-prioritize through an open-ended priority DSL.

## Routes

Routes are only declarations needed for host routing/auth classification. The plugin owns internal request semantics/body schemas; V1 does not embed an OpenAPI-like route schema language in the descriptor.

Allowed mounts:

- `extension-api` → under `/extensions/{plugin-id}/api/*`
- `public` → under `/plugins/{plugin-id}/...`

Allowed auth classes: `public`, `customer`, `merchant`.

## Navigation/UI

A plugin with UI may declare one simple navigation entry under the BC Extensions area. The plugin owns all routes beneath its own UI namespace. Backend-only plugins do not get navigation by default.

## Config schema

Supported descriptor config types:

- string
- secret
- boolean
- integer
- number
- enum

Supported basic constraints/defaults include required, default, min/max, min_length/max_length and pattern as meaningful for the type. Nested objects/lists are intentionally excluded from generic descriptor config V1.

Removing a config field in a later version does not automatically delete the stored old value. Adding a new required field with no value makes activation/configuration incomplete until supplied.

## Jobs and schedules

Named job definitions are stable within the plugin release. Queue records retain the job type/version context that created them; an upgrade that removes an outstanding job handler makes old queued work incompatible/dead-lettered rather than silently remapping it.

Recurring schedules are declared statically. Duplicate schedule IDs are invalid. Upgrades reconcile future schedule definitions after successful activation.

## CLI

Commands use simple typed args parsed/validated by BC. Plugin returns structured output so BC can render normal text or `--json`.

## Events

Subscriptions declare both BC-owned event type and event schema version and are deduplicated during validation. A subscription is unique within a plugin by `(type, version)`. Core event names/versions are BC-owned; plugins cannot publish new canonical BC event types through the descriptor. Durable fan-out uses the source event ID plus plugin ID/type/version as its idempotency identity.
