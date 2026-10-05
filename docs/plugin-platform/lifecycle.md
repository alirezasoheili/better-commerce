# Plugin lifecycle and recovery state machine

## Operator-visible state

```text
                    enable / startup activation
        ┌─────────────────────────────────────────────┐
        │                                             ▼
   disabled                                      active
        ▲                                             │
        │                                             │ activation/runtime failure
        │ disable                                     ▼
        └────────────────────────────────────── unavailable
                                                      │
                                                      │ retry activation / enable / restart
                                                      └──────────────► active
```

`installed` is registry/package presence, not a separate operator runtime state. Install/upgrade/remove have transient operation states internally.

## Startup activation

```text
managed package
  ↓
validate digest/descriptor/package paths
  ↓
validate BC range + capabilities + WIT imports/exports
  ↓
required config complete?
  ↓
run pending checksum-tracked migrations
  ↓
backend.wasm present?
  ├─ no ────────────────────────────────► register valid UI/package declarations → active
  └─ yes
       ↓
    instantiate Wasmtime component
       ↓
    initialize()
       ↓
    health()
  ├─ unhealthy ─────────────────────────► unavailable
  ├─ error/trap/timeout ─────────────────► unavailable
  └─ healthy/degraded
        ↓
register routes/extensions/events/jobs/schedules/UI
        ↓
      active
```

Any activation failure is plugin-local. BC process startup continues.

## Runtime invocation

```text
incoming invocation
  ↓
plugin active and circuit closed?
  ├─ no → fail fast with stable host error
  └─ yes
       ↓
 acquire per-plugin serialized execution slot
       ↓
 invoke component under global timeout/resource policy
       ├─ success → release slot
       ├─ normal plugin error → apply extension/job/event-specific semantics
       └─ timeout/trap
             ↓
          discard instance
             ↓
          record failure / circuit accounting
             ↓
          recreate component
             ↓
          initialize()
             ├─ success → callable again if circuit policy permits
             └─ failure → unavailable
```

A health check is not required after every trap recreation; `initialize()` is the required reactivation step. Explicit health remains a startup/operator check.

## Disable

```text
disable requested
  ↓
stop accepting new invocations
  ↓
allow current invocation to finish until global timeout
  ↓
best-effort shutdown() with short deadline
  ↓
discard component
  ↓
mark disabled
```

Queued jobs/events remain durable. Their TTL keeps advancing.

## Config update

```text
candidate config
  ↓
descriptor/basic validation
  ↓
backend.wasm present?
  ├─ no → descriptor validation is sufficient
  └─ yes
       ↓
    active instance available?
       ├─ yes → serialized validate_config(candidate) on active instance
       └─ no  → instantiate fresh transient validation instance → validate_config(candidate) → discard it
  ↓
valid?
  ├─ no → reject, stored config unchanged
  └─ yes
       ↓
    persist values / encrypted secrets
       ↓
    active runtime instance exists?
       ├─ no → next activation consumes stored config
       └─ yes → config_changed()
                    ├─ success → remain active
                    └─ failure → record plugin error; no automatic config rollback
```

## Install

Install is an atomic operator operation for package/registry state:

```text
local ZIP
  ↓ validate
copy to temporary managed location
  ↓
atomically publish managed package
  ↓
registry desired_enabled=true
  ↓
restart-required
```

No migrations or component activation occur in the install CLI path.

## Upgrade

```text
validate/copy new package
  ↓
replace managed installed release
  ↓
restart-required
  ↓
normal activation path
```

If a migration commits and later activation fails, the migration remains applied and the plugin is unavailable. V1 does not auto-rollback package/database state.

Old queued event subscriptions removed by the new release become obsolete/cancelled. Old queued job types no longer implemented become incompatible/dead-lettered.

## Remove

Normal remove:

```text
disable/unload → remove package/runtime registration → preserve DB/config/data/migration history
```

Destructive remove:

```text
disable/unload → drain/resolve active work → explicit delete of plugin DB/config/data/history as removal contract defines
```

## BC upgrade preflight

Before a BC upgrade mutates core state, validate every enabled installed plugin's declared BC range and required WIT interfaces against the target BC release. Incompatible enabled plugins block the upgrade until upgraded or disabled.

## Crash recovery

### Durable jobs

Running jobs hold a lease/heartbeat. After BC crashes, expired leases become reclaimable and are retried under at-least-once semantics.

### Events

Outbox publication stays post-commit and preserves ADR 0003's acceptance boundary. The plugin `EventDelivery` adapter first commits idempotent downstream plugin-delivery rows for matching subscriptions, then acknowledges the core outbox. A plugin worker later invokes WASM. Plugin-delivery completion is recorded only after handler success; retry/dead-letter state belongs to the downstream delivery row, and replay preserves the original source event ID. A broken plugin therefore does not keep the core module outbox unpublished after durable fan-out.

### External provider ambiguity

If a plugin invoked an external provider and BC/plugin crashed before recording the result, the operation remains indeterminate. Stable BC operation/reference IDs are used for provider idempotency and/or reconciliation; recovery does not guess decline or blindly duplicate the call.
