# Issue tracker: GitHub

Issues and specs for this repository live in GitHub Issues. Use the `gh` CLI from the repository checkout.

## Conventions

- Create issues with `gh issue create`.
- Read issues with `gh issue view <number> --comments`.
- List issues with `gh issue list`, using state and label filters as needed.
- Specs and tickets created by engineering skills receive the `ready-for-agent` label.
- Pull requests are not a request surface.

The repository is identified from its Git remote.

When a skill says to publish a spec or ticket, create a GitHub issue. When a skill says to fetch a ticket, read it with `gh issue view <number> --comments`.
