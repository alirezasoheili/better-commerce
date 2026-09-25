# Better Commerce

Better Commerce is a Rust commerce platform built as a DDD modular monolith
with extractable bounded-context modules.

## Hard architecture invariants

- Modules own their data.
- A module must never query another module's database schema directly.
- Cross-module synchronous communication goes through typed ports.
- Local and remote adapters implement the same application-facing port.
- Protobuf/gRPC is a transport boundary, not a domain boundary.
- Transport DTOs must not leak into domain/application code.
- Durable business events use a transactional outbox.
- PostgreSQL is authoritative business state.
- Redis may accelerate or coordinate but is not authoritative business state.
- Browser-facing APIs use HTTP/JSON.
- Extracting a module must not require changing its consumers' application logic.
- Accepted ADRs must not be silently contradicted.

## Context policy

Do not read all project documentation.

For a task:
1. Read the applicable AGENTS.md files.
2. Read the task/ticket.
3. Read only ADRs referenced by the task or relevant module context.
4. Load other documentation only when the implementation crosses that boundary.

Prefer code and tests over duplicating implementation details in documentation.

## Change policy

If implementation requires contradicting an accepted architectural decision,
stop and surface the conflict rather than silently redesigning the system.

## Verification

Run the smallest relevant test set during development.
Run required package/workspace checks before completion.

## Agent skills

### Issue tracker

Issues and specs live in GitHub Issues; use the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the default triage labels for this repository. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: use the root `CONTEXT.md` glossary, `docs/context-map.yaml` for task-specific document routing, and `docs/adr/` for system decisions. See `docs/agents/domain.md`.

Follow the ticket completion workflow in `docs/agents/issue-tracker.md`.