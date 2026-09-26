# M0-04 PostgreSQL setup

The migration command is `cargo run -p better-commerce-core --bin migrate`. Set
`OPERATIONS_DATABASE_URL` and `READINESS_DATABASE_URL` to the same installation
database. When `example` is enabled, also set `EXAMPLE_RUNTIME_DATABASE_URL` and
`DISPATCHER_DATABASE_URL` for its module runtime and outbox dispatcher identities.
The operations login needs permission to create roles and schemas. The runner
creates distinct runtime/readiness logins from their URLs, applies the shared
and example migration sets under the operations login, and grants only their
respective schema access. The example runtime receives module table access, while
the dispatcher receives only `SELECT` and `UPDATE` on `bc_example.outbox_events`.
Keep the operations URL out of the server process; the server uses only the
example runtime and readiness URLs.
The runtime usernames must match the database-specific names returned by
`better_commerce_core::database::scoped_role_names(database_name)`; role reuse
across installation databases is rejected.

For local PostgreSQL integration tests on Windows, run
`./deploy/with-test-postgres.ps1 -Body { cargo test --workspace }`. It creates a
fresh temporary PostgreSQL cluster and sets `BC_TEST_ADMIN_DATABASE_URL` for
the test process. Set `BC_POSTGRES_BIN` if PostgreSQL binaries are not at
`F:\postgres\bin`.

On other platforms, start `deploy/test-postgres.compose.yaml` and set
`BC_TEST_ADMIN_DATABASE_URL` to its test-only administrator URL before running
the same Cargo tests. Each test creates and removes a unique test database and
test-scoped roles. GitHub Actions supplies its own PostgreSQL service and URL.
