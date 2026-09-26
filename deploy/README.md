# M0-04 PostgreSQL setup

## Local first install

Prerequisites: Docker with the Compose plugin. The checked-in [`manifest.yaml`](../manifest.yaml)
contains nonsecret settings and local secret references. Secret references use either
`env: VARIABLE_NAME` or `file: ./relative/path`; file paths are resolved beside the
manifest and one trailing line ending is removed.

Set `BC_OPERATIONS_PASSWORD`, `BC_EXAMPLE_PASSWORD`, `BC_DISPATCHER_PASSWORD`, and
`BC_READINESS_PASSWORD` in the environment (or change the manifest references to
files), then build and run the `bc` binary:

```sh
cargo build --locked -p better-commerce-core --bin bc
cargo run --locked -p better-commerce-core --bin bc -- reconcile --manifest manifest.yaml
```

The command validates the manifest and module configuration before it invokes Docker,
starts PostgreSQL 18, runs the owned migrations with the operations identity, starts
the API with scoped runtime/readiness credentials, and waits up to 90 seconds for
`GET /readyz` to return HTTP 200. The API is published on the manifest's loopback-only
`local.http_port`. Compose project names are derived deterministically from
`local.installation_id`; the named PostgreSQL volume persists installation data.
Compose state is described by `deploy/compose.yaml`; no generated file contains
resolved secret values. Normal reconciliation never removes volumes.

Minimal manifest shape (the root `manifest.yaml` is also a runnable example once its
environment variables are set):

```yaml
release: 0.1.0
deployment_mode: self_hosted
modules:
  example:
    version: 0.1.0
    configuration:
      label: Example
local:
  installation_id: example-local
  database_name: bc_example_local
  http_port: 3000
  secrets:
    operations_password: { env: BC_OPERATIONS_PASSWORD }
    example_password: { env: BC_EXAMPLE_PASSWORD }
    dispatcher_password: { env: BC_DISPATCHER_PASSWORD }
    readiness_password: { env: BC_READINESS_PASSWORD }
```

On success `bc` prints the `/readyz` URL. PostgreSQL is only reachable on the
installation's private Compose network.

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
