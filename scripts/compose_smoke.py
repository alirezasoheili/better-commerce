#!/usr/bin/env python3
"""Exercise the real bc → Compose → migrations → API first-install path."""

import hashlib
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


def run(command, *, env=None, capture=False):
    return subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        check=False,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
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
        manifest = Path(directory) / "manifest.yaml"
        manifest.write_text(
            f"""release: 0.1.0
deployment_mode: self_hosted
modules:
  example:
    version: 0.1.0
    configuration:
      label: Compose smoke
local:
  installation_id: {installation_id}
  database_name: {database}
  http_port: {port}
  secrets:
    operations_password: {{ env: BC_SMOKE_OPERATIONS_PASSWORD }}
    example_password: {{ env: BC_SMOKE_EXAMPLE_PASSWORD }}
    dispatcher_password: {{ env: BC_SMOKE_DISPATCHER_PASSWORD }}
    readiness_password: {{ env: BC_SMOKE_READINESS_PASSWORD }}
""",
            encoding="utf-8",
        )
        env = os.environ.copy()
        sentinels = {}
        for name in (
            "BC_SMOKE_OPERATIONS_PASSWORD",
            "BC_SMOKE_EXAMPLE_PASSWORD",
            "BC_SMOKE_DISPATCHER_PASSWORD",
            "BC_SMOKE_READINESS_PASSWORD",
        ):
            value = "TOP_SECRET_SHOULD_NEVER_APPEAR_" + secrets.token_urlsafe(24)
            sentinels[value] = name
            env[name] = value

        try:
            result = run(
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
                capture=True,
            )
            combined = (result.stdout or "") + (result.stderr or "")
            if any(secret in combined for secret in sentinels):
                raise RuntimeError("secret value appeared in captured CLI output")
            if result.returncode:
                raise RuntimeError("bc reconciliation failed; inspect the nonsecret CI job diagnostics")
            if "is ready at" not in result.stdout:
                raise RuntimeError("bc did not report successful readiness")

            with urllib.request.urlopen(f"http://127.0.0.1:{port}/readyz", timeout=5) as response:
                if response.status != 200:
                    raise RuntimeError("independent /readyz check did not return HTTP 200")
            print(f"Compose first-install smoke passed for {database} on port {port}.")
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
            cleanup = run(
                ["docker", "compose", "--project-name", project, "--file", str(COMPOSE), "down", "--volumes", "--remove-orphans"],
                env=cleanup_env,
                capture=True,
            )
            if cleanup.returncode:
                raise RuntimeError("failed to clean up the disposable Compose smoke project")


if __name__ == "__main__":
    main()
