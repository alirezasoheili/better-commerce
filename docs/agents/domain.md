# Domain Docs

## Before exploring

Read the root `CONTEXT.md` glossary, `docs/context-map.yaml` routing metadata, and relevant decisions in `docs/adr/`. If any of these files are empty or do not exist yet, proceed without treating that as a problem.

## Responsibilities

- `CONTEXT.md` is the concise, human- and agent-readable domain glossary and source for ubiquitous language. Keep it free of implementation details.
- `docs/context-map.yaml` is a small machine-readable index for routing agents to the task-relevant contexts, documents, ADRs, and specs. Do not copy the glossary or ADR contents into it.
- `docs/adr/` records concise, durable system decisions.

Keep the glossary and routing index small. Create and populate them lazily as the architecture becomes concrete.

Use glossary terms consistently. Surface conflicts with accepted ADRs instead of silently overriding them.
