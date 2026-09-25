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

## Ticket completion workflow

For implementation tickets:

1. Work on a dedicated branch/worktree.
2. Run the ticket's required verification.
3. Run `code-review` and resolve blocking findings.
4. Commit the completed ticket.
5. Push the branch to `origin`.
6. Open a pull request against `main` with:
   - the ticket title;
   - a concise summary;
   - verification results;
   - `Closes #<issue-number>`.
7. Check CI with `gh pr checks`.
8. Do not merge automatically unless explicitly instructed.
9. Report the PR URL, commit SHA, CI status, and any remaining blockers.

Do not start the next ticket from the current branch.
