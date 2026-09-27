#!/usr/bin/env python3
"""Exercise real repeated bc reconciliation and safe initialized-module removal."""

import hashlib
import json
import os
import secrets
import socket
import subprocess
import tempfile
import urllib.request
import uuid
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
COMPOSE = ROOT / "deploy" / "compose.yaml"
SECRET_ENV = (
    "BC_SMOKE_OPERATIONS_PASSWORD",
    "BC_SMOKE_EXAMPLE_PASSWORD",
    "BC_SMOKE_DISPATCHER_PASSWORD",
    "BC_SMOKE_READINESS_PASSWORD",
)


def run(command, *, env, capture=True):
    return subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        check=False,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )


def compose(project, env, *args):
    return run(
        ["docker", "compose", "--project-name", project, "--file", str(COMPOSE), *args],
        env=env,
    )


def require_success(result, operation):
    if result.returncode:
        raise RuntimeError(f"{operation} failed with exit status {result.returncode}")


def run_reconcile(manifest, env):
    return run(
        [
            "cargo",
            "run",
            "--locked",
            "-p",
            "better-commerce-core",
            "--bin",
            "bc",
            "--",
            "reconcile",
            "--manifest",
            str(manifest),
        ],
        env=env,
    )


def ensure_no_secret_output(result, secrets_by_value):
    output = (result.stdout or "") + (result.stderr or "")
    if any(secret in output for secret in secrets_by_value):
        raise RuntimeError("secret value appeared in captured reconciliation output")
    return output


def container_id(project, env, service):
    result = compose(project, env, "ps", "--quiet", service)
    require_success(result, f"look up {service} container")
    value = result.stdout.strip()
    if not value:
        raise RuntimeError(f"Compose did not report a running {service} container")
    return value


def volume_name(project, env):
    postgres_id = container_id(project, env, "postgres")
    result = run(
        [
            "docker",
            "inspect",
            "--format={{range .Mounts}}{{if eq .Destination \"/var/lib/postgresql\"}}{{.Name}}{{end}}{{end}}",
            postgres_id,
        ],
        env=env,
    )
    require_success(result, "inspect PostgreSQL volume")
    name = result.stdout.strip()
    if not name:
        raise RuntimeError("PostgreSQL is not using its named data volume")
    return name


def sql(project, database, env, statement):
    result = compose(
        project,
        env,
        "exec",
        "--no-TTY",
        "--env",
        "PGPASSWORD=" + env["BC_SMOKE_OPERATIONS_PASSWORD"],
        "postgres",
        "psql",
        "-X",
        "-v",
        "ON_ERROR_STOP=1",
        "-U",
        "postgres",
        "-d",
        database,
        "-At",
        "-c",
        statement,
    )
    require_success(result, "query disposable PostgreSQL state")
    return result.stdout.strip()


def json_sql(project, database, env, statement):
    return json.loads(sql(project, database, env, statement))


def snapshot(project, database, env, event_id, volume):
    event_literal = "'" + event_id.replace("'", "''") + "'"
    return {
        "business": json_sql(
            project,
            database,
            env,
            "SELECT json_build_array(id, label)::text FROM bc_example.example_records "
            "WHERE label LIKE 'repeat-smoke-%' ORDER BY id",
        ),
        "event": json_sql(
            project,
            database,
            env,
            "SELECT json_build_object("
            "'event_id', event_id::text, 'event_type', event_type, 'event_version', event_version, "
            "'aggregate_type', aggregate_type, 'aggregate_id', aggregate_id, "
            "'aggregate_sequence', aggregate_sequence, 'payload_schema_version', payload_schema_version, "
            "'payload', payload, 'delivered_at', delivered_at, 'dead_lettered_at', dead_lettered_at, "
            "'dead_letter_reason', dead_letter_reason)::text FROM bc_example.outbox_events WHERE event_id = "
            + event_literal,
        ),
        "shared_migrations": json_sql(
            project,
            database,
            env,
            "SELECT COALESCE(json_agg(json_build_array(version, success, encode(checksum, 'hex')) "
            "ORDER BY version), '[]'::json)::text FROM bc_shared._sqlx_migrations",
        ),
        "example_migrations": json_sql(
            project,
            database,
            env,
            "SELECT COALESCE(json_agg(json_build_array(version, success, encode(checksum, 'hex')) "
            "ORDER BY version), '[]'::json)::text FROM bc_example._sqlx_migrations",
        ),
        "example_schema_exists": sql(
            project, database, env, "SELECT to_regnamespace('bc_example') IS NOT NULL"
        )
        == "t",
        "postgres_volume": volume,
    }


def assert_pending_event(state, event_id, record_id, label):
    if len(state["business"]) != 1 or state["business"][0] != [record_id, label]:
        raise RuntimeError("distinctive business record did not match its retained snapshot")
    event = state["event"]
    expected_payload = {"schema_version": 1, "record": {"id": record_id, "label": label}}
    if not event or event != {
        "event_id": event_id,
        "event_type": "example.record_created",
        "event_version": 1,
        "aggregate_type": "example_record",
        "aggregate_id": record_id,
        "aggregate_sequence": 73,
        "payload_schema_version": 1,
        "payload": expected_payload,
        "delivered_at": None,
        "dead_lettered_at": None,
        "dead_letter_reason": None,
    }:
        raise RuntimeError("distinctive outbox event was not retained exactly as a pending event")
    if not state["example_schema_exists"]:
        raise RuntimeError("initialized example schema disappeared")


def ready(port):
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/readyz", timeout=10) as response:
        if response.status != 200:
            raise RuntimeError("independent /readyz check did not return HTTP 200")


def write_manifest(path, installation_id, database, port, label=None):
    modules = (
        "modules: {}\n"
        if label is None
        else f"modules:\n  example:\n    version: 0.1.0\n    configuration:\n      label: {label}\n"
    )
    path.write_text(
        "release: 0.1.0\n"
        "deployment_mode: self_hosted\n"
        + modules
        + f"local:\n  installation_id: {installation_id}\n  database_name: {database}\n  http_port: {port}\n"
        + "  secrets:\n"
        + "    operations_password: { env: BC_SMOKE_OPERATIONS_PASSWORD }\n"
        + "    example_password: { env: BC_SMOKE_EXAMPLE_PASSWORD }\n"
        + "    dispatcher_password: { env: BC_SMOKE_DISPATCHER_PASSWORD }\n"
        + "    readiness_password: { env: BC_SMOKE_READINESS_PASSWORD }\n",
        encoding="utf-8",
    )


def main():
    unique = uuid.uuid4().hex[:16]
    installation_id = f"smoke-{unique}"
    database = f"bc_smoke_{unique}"
    project = f"bc-{installation_id}-{hashlib.sha256(installation_id.encode()).hexdigest()[:16]}"
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]

    with tempfile.TemporaryDirectory(prefix="bc-compose-smoke-") as directory:
        directory = Path(directory)
        manifest = directory / "manifest.yaml"
        removal_manifest = directory / "removal-manifest.yaml"
        write_manifest(manifest, installation_id, database, port, "Before")
        write_manifest(removal_manifest, installation_id, database, port)

        env = os.environ.copy()
        secret_values = {}
        for name in SECRET_ENV:
            value = "TOP_SECRET_SHOULD_NEVER_APPEAR_" + secrets.token_urlsafe(24)
            secret_values[value] = name
            env[name] = value

        try:
            first = run_reconcile(manifest, env)
            first_output = ensure_no_secret_output(first, secret_values)
            if first.returncode:
                raise RuntimeError(
                    "first bc reconciliation failed after secret-sentinel scan:\n"
                    + first_output[-3000:]
                )
            if "is ready at" not in (first.stdout or ""):
                raise RuntimeError("bc did not report first-install readiness")
            ready(port)
            print("Compose first-install readiness passed.")

            record_id = secrets.randbelow(800_000_000) + 100_000_000
            record_label = f"repeat-smoke-{unique}"
            event_id = str(uuid.uuid4())
            payload = json.dumps(
                {"schema_version": 1, "record": {"id": record_id, "label": record_label}},
                separators=(",", ":"),
            )
            escaped_label = record_label.replace("'", "''")
            escaped_payload = payload.replace("'", "''")
            seed = (
                "BEGIN; "
                "INSERT INTO bc_example.example_records (id, label) OVERRIDING SYSTEM VALUE VALUES ("
                f"{record_id}, '{escaped_label}'); "
                "INSERT INTO bc_example.outbox_events "
                "(event_id, event_type, event_version, aggregate_type, aggregate_id, aggregate_sequence, "
                "payload_schema_version, payload, delivered_at, dead_lettered_at, dead_letter_reason) VALUES ("
                f"'{event_id}', 'example.record_created', 1, 'example_record', {record_id}, 73, 1, "
                f"'{escaped_payload}'::jsonb, NULL, NULL, NULL); COMMIT"
            )
            sql(project, database, env, seed)
            volume = volume_name(project, env)
            before_repeat = snapshot(project, database, env, event_id, volume)
            assert_pending_event(before_repeat, event_id, record_id, record_label)
            if len(before_repeat["shared_migrations"]) != 1 or len(before_repeat["example_migrations"]) != 3:
                raise RuntimeError("migration history snapshot did not contain expected applied migrations")

            repeated = run_reconcile(manifest, env)
            ensure_no_secret_output(repeated, secret_values)
            if repeated.returncode:
                raise RuntimeError("unchanged repeat reconciliation failed (secret output suppressed)")
            ready(port)
            after_repeat = snapshot(project, database, env, event_id, volume_name(project, env))
            assert_pending_event(after_repeat, event_id, record_id, record_label)
            if after_repeat != before_repeat:
                raise RuntimeError("unchanged reconciliation changed persisted state or volume identity")
            print("Unchanged reconciliation preserved business, migration, outbox, and volume state.")

            api_before_config = container_id(project, env, "api")
            write_manifest(manifest, installation_id, database, port, "After")
            changed = run_reconcile(manifest, env)
            ensure_no_secret_output(changed, secret_values)
            if changed.returncode:
                raise RuntimeError("supported configuration reconciliation failed (secret output suppressed)")
            api_after_config = container_id(project, env, "api")
            if api_after_config == api_before_config:
                raise RuntimeError("API container identity did not change after configuration update")
            ready(port)
            after_config = snapshot(project, database, env, event_id, volume_name(project, env))
            assert_pending_event(after_config, event_id, record_id, record_label)
            if after_config != before_repeat:
                raise RuntimeError("configuration reconciliation changed persisted state or volume identity")
            print("Supported configuration change recreated the API and preserved state.")

            api_before_removal = container_id(project, env, "api")
            removal = run_reconcile(removal_manifest, env)
            removal_output = ensure_no_secret_output(removal, secret_values)
            if removal.returncode == 0:
                raise RuntimeError("removing an initialized example module unexpectedly succeeded")
            if "initialized module 'example' cannot be removed in M0" not in removal_output:
                raise RuntimeError("removal failure was not actionable or did not identify the unsupported transition")
            if container_id(project, env, "api") != api_before_removal:
                raise RuntimeError("API was recreated into the module-less desired state")
            ready(port)
            after_removal = snapshot(project, database, env, event_id, volume_name(project, env))
            assert_pending_event(after_removal, event_id, record_id, record_label)
            if after_removal != before_repeat:
                raise RuntimeError("rejected module removal changed persisted state or volume identity")
            print("Initialized-module removal was rejected safely; prior API remained ready.")
            print("Compose repeat-reconciliation smoke passed.")
        finally:
            cleanup_env = env.copy()
            cleanup_env.update(
                {
                    "BC_OPERATIONS_PASSWORD": "cleanup-only",
                    "BC_EXAMPLE_PASSWORD": "cleanup-only",
                    "BC_DISPATCHER_PASSWORD": "cleanup-only",
                    "BC_READINESS_PASSWORD": "cleanup-only",
                    "BC_DATABASE_NAME": database,
                    "BC_HTTP_PORT": str(port),
                    "BC_MANIFEST_HOST_PATH": str(manifest).replace("\\", "/"),
                    "OPERATIONS_DATABASE_URL": "postgres://postgres:cleanup-only@postgres:5432/cleanup",
                    "EXAMPLE_RUNTIME_DATABASE_URL": "postgres://example:cleanup-only@postgres:5432/cleanup",
                    "DISPATCHER_DATABASE_URL": "postgres://dispatcher:cleanup-only@postgres:5432/cleanup",
                    "READINESS_DATABASE_URL": "postgres://ready:cleanup-only@postgres:5432/cleanup",
                }
            )
            cleanup = compose(project, cleanup_env, "down", "--volumes", "--remove-orphans")
            if cleanup.returncode:
                raise RuntimeError("failed to clean up disposable Compose project and volume")


if __name__ == "__main__":
    main()
