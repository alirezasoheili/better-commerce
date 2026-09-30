# Reference plugin acceptance matrix

The reference plugins are architecture proofs, not marketplace products. CI may use deterministic local fake external providers.

| Capability | Payment | Gold pricing | SMS | UI/data |
| --- | :---: | :---: | :---: | :---: |
| WASM lifecycle/health | ✓ | ✓ | ✓ | ✓ |
| Config + encrypted secret | ✓ | ✓ | ✓ | ✓ |
| Outbound HTTP | ✓ | ✓ | ✓ | optional |
| Public callback | ✓ |  | optional webhook |  |
| Single-provider extension | ✓ |  |  |  |
| Multi-contributor extension |  | ✓ |  |  |
| Core domain host API | ✓ | ✓ | ✓ | ✓ |
| Direct core read compatibility fixture | optional | ✓ | optional | ✓ |
| Plugin migrations/tables | optional | optional | optional | ✓ |
| Durable event delivery | optional | optional | ✓ | optional |
| Durable job/retry/dead-letter | reconciliation | refresh optional | ✓ | ✓ |
| Idempotent external side effect | ✓ | n/a | ✓ | n/a |
| Lost-response/indeterminate recovery | ✓ | fetch retry | ✓ | job retry |
| UI + frontend SDK | config/status optional | optional | optional | ✓ |
| Upload/download streaming |  |  |  | ✓ |
| CLI command | ✓ test connection | ✓ refresh/test | ✓ test send | ✓ maintenance |
| Backup/restore | ✓ | ✓ | ✓ | ✓ |
| Upgrade/config schema change | ✓ | ✓ | ✓ | ✓ |

## Payment gateway proof

Must demonstrate:

- structured method appears only when configured/eligible;
- BC-generated stable payment operation/reference is used for provider interaction;
- callback route verifies provider signature/secret inside plugin;
- duplicate callback cannot duplicate canonical BC transition;
- crash after provider success but before plugin return produces indeterminate recovery, not blind retry/decline;
- timeout/trap recreates plugin without losing durable recovery facts;
- plugin failure cannot mutate Orders/Payments outside BC domain APIs;
- disabling plugin makes its payment method unavailable without crashing BC.

## Gold/dynamic pricing proof

Must demonstrate:

- plugin participates as a multi-contributor at BC-defined pricing checkpoints;
- returns structured adjustment, never overwrites total directly;
- adjustment identity is namespaced by plugin ID;
- external rate lookup uses outbound HTTP and can cache only as disposable optimization;
- authoritative purchase submit requotes;
- changed total requires customer reconfirmation;
- required contributor failure blocks authoritative quote according to extension contract;
- restart/reinstance does not change correctness because durable state/rate policy is not only in WASM memory.

## SMS proof

Must demonstrate:

- core commits business event/outbox before plugin handling;
- event handler enqueues durable send job;
- stable event/job/reference ID prevents duplicate send where provider supports idempotency or plugin records it;
- BC crash after provider accepts send but before completion record is safe under at-least-once rules;
- retries/backoff/dead-letter/manual retry work;
- disabling plugin retains queue; TTL can expire while disabled;
- re-enable resumes eligible work;
- failure for one aggregate does not block unrelated aggregate event progression.

## UI/data proof

Must demonstrate:

- plugin-owned migration with checksum history;
- explicit plugin transaction;
- authenticated `/extensions/{id}/api/*` route receives PluginContext;
- React/Vite static UI deep-link refresh works;
- backend-unavailable state can be shown while static UI remains loadable;
- package assets immutable-cache correctly and new version does not reuse stale JS;
- upload streams to scoped storage and WASM receives file ref;
- export returns file ref and BC streams download after invocation ends;
- job progress is visible through host-owned job API/SSE;
- normal uninstall preserves DB/files; destructive remove deletes them only explicitly;
- backup/restore reproduces exact package/data;
- config upgrade preserves existing values and handles new required field as config-incomplete/unavailable.

## Cross-plugin acceptance

Run all four together and prove:

- one plugin's trap/circuit/unavailability does not stop BC or unrelated plugins;
- per-plugin serialized execution does not serialize the entire platform;
- plugin lifecycle mutation locks prevent concurrent conflicting operations on the same plugin;
- contributor order/default provider selection is deterministic;
- BC remains deployable with only its executable, PostgreSQL and plugin package/data state;
- M0/M1 module ownership and outbox regression tests still pass.
