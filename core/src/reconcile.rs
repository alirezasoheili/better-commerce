//! Caller-independent first-install reconciliation for a local Docker Compose installation.

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant},
};

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use sha2::Digest;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use crate::{
    composition::compose_modules,
    database::scoped_role_names,
    manifest::{LocalDeployment, SecretReference, parse_and_validate, supported_release_metadata},
};

const READY_TIMEOUT: Duration = Duration::from_secs(90);
const READY_INTERVAL: Duration = Duration::from_secs(1);
const DOCKER_COMPOSE: &str = "docker compose";

#[derive(Clone, Debug)]
pub struct ReconcileRequest {
    pub manifest_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconcileReport {
    pub installation_id: String,
    pub database_name: String,
    pub compose_project: String,
    pub ready_url: String,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ReconcileError {
    phase: &'static str,
    message: String,
}

impl ReconcileError {
    fn new(phase: &'static str, message: impl Into<String>) -> Self {
        Self {
            phase,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ReconcileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "reconciliation {} failed: {}", self.phase, self.message)
    }
}

impl std::fmt::Debug for ReconcileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReconcileError")
            .field("phase", &self.phase)
            .field("message", &self.message)
            .finish()
    }
}

impl std::error::Error for ReconcileError {}

/// Validate all desired/module/local configuration, resolve local secrets, then apply
/// PostgreSQL, owned migrations, the API service, and `/readyz` in that order.
pub async fn reconcile(request: ReconcileRequest) -> Result<ReconcileReport, ReconcileError> {
    reconcile_with_environment(request, |name| std::env::var(name)).await
}

async fn reconcile_with_environment<F>(
    request: ReconcileRequest,
    get_env: F,
) -> Result<ReconcileReport, ReconcileError>
where
    F: Fn(&str) -> Result<String, std::env::VarError>,
{
    let manifest_path = std::fs::canonicalize(&request.manifest_path)
        .map_err(|_| ReconcileError::new("validate", "could not read manifest file"))?;
    let source = std::fs::read_to_string(&manifest_path)
        .map_err(|_| ReconcileError::new("validate", "could not read manifest file"))?;
    let validated = parse_and_validate(&source, &supported_release_metadata())
        .map_err(|_| ReconcileError::new("validate", "manifest is invalid"))?;

    // Generic validation does not validate opaque module configuration. Compose before
    // resolving or applying anything so bad module configuration has no side effects.
    compose_modules(validated.clone())
        .map_err(|error| ReconcileError::new("validate", error.to_string()))?;

    let local = validated.local.ok_or_else(|| {
        ReconcileError::new(
            "validate",
            "manifest is missing local deployment configuration",
        )
    })?;
    validate_local_deployment(&local)?;
    let secrets = resolve_secrets(
        &local,
        manifest_path.parent().unwrap_or(Path::new(".")),
        &get_env,
    )?;

    let project = compose_project_name(&local.installation_id);
    let compose_file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../deploy/compose.yaml");
    let compose_file = std::fs::canonicalize(compose_file)
        .map_err(|_| ReconcileError::new("plan", "Compose definition is unavailable"))?;
    let manifest_host_path = manifest_path.to_string_lossy().replace('\\', "/");
    let database_name = local.database_name.clone();
    let (example_role, dispatcher_role, readiness_role) = scoped_role_names(&database_name);
    let host = "postgres";
    let ops_url = database_url(
        "postgres",
        &secrets.operations_password,
        host,
        &database_name,
    );
    let example_url = database_url(
        &example_role,
        &secrets.example_password,
        host,
        &database_name,
    );
    let dispatcher_url = database_url(
        &dispatcher_role,
        &secrets.dispatcher_password,
        host,
        &database_name,
    );
    let readiness_url = database_url(
        &readiness_role,
        &secrets.readiness_password,
        host,
        &database_name,
    );
    let mut environment = BTreeMap::new();
    environment.insert(
        "BC_INSTALLATION_ID".to_owned(),
        local.installation_id.clone(),
    );
    environment.insert("BC_DATABASE_NAME".to_owned(), database_name.clone());
    environment.insert("BC_HTTP_PORT".to_owned(), local.http_port.to_string());
    environment.insert("BC_MANIFEST_HOST_PATH".to_owned(), manifest_host_path);
    environment.insert(
        "BC_OPERATIONS_PASSWORD".to_owned(),
        secrets.operations_password,
    );
    environment.insert("BC_EXAMPLE_PASSWORD".to_owned(), secrets.example_password);
    environment.insert(
        "BC_DISPATCHER_PASSWORD".to_owned(),
        secrets.dispatcher_password,
    );
    environment.insert(
        "BC_READINESS_PASSWORD".to_owned(),
        secrets.readiness_password,
    );
    environment.insert("OPERATIONS_DATABASE_URL".to_owned(), ops_url);
    environment.insert("EXAMPLE_RUNTIME_DATABASE_URL".to_owned(), example_url);
    environment.insert("DISPATCHER_DATABASE_URL".to_owned(), dispatcher_url);
    environment.insert("READINESS_DATABASE_URL".to_owned(), readiness_url);

    let context = ComposeContext {
        file: compose_file,
        project,
        environment,
    };
    run_compose(
        &context,
        "apply PostgreSQL",
        &["up", "--detach", "--build", "postgres"],
    )?;
    run_compose(
        &context,
        "run owned migrations",
        &["run", "--rm", "migrate"],
    )?;
    run_compose(&context, "start API", &["up", "--detach", "api"])?;

    let ready_url = format!("http://127.0.0.1:{}/readyz", local.http_port);
    verify_ready(&ready_url).await?;
    Ok(ReconcileReport {
        installation_id: local.installation_id,
        database_name,
        compose_project: context.project,
        ready_url,
    })
}

struct ResolvedSecrets {
    operations_password: String,
    example_password: String,
    dispatcher_password: String,
    readiness_password: String,
}

impl ResolvedSecrets {
    fn resolve<F>(
        reference: &SecretReference,
        manifest_dir: &Path,
        get_env: &F,
    ) -> Result<String, ReconcileError>
    where
        F: Fn(&str) -> Result<String, std::env::VarError>,
    {
        let value = match reference {
            SecretReference::Environment { env } => {
                if env.is_empty() || !env.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    return Err(ReconcileError::new(
                        "validate",
                        "secret environment reference is invalid",
                    ));
                }
                get_env(env).map_err(|_| {
                    ReconcileError::new(
                        "validate",
                        "required secret environment variable is unavailable",
                    )
                })?
            }
            SecretReference::File { file } => {
                let path = if file.is_absolute() {
                    file.clone()
                } else {
                    manifest_dir.join(file)
                };
                std::fs::read_to_string(path)
                    .map_err(|_| {
                        ReconcileError::new("validate", "required secret file is unavailable")
                    })?
                    .trim_end_matches(['\r', '\n'])
                    .to_owned()
            }
        };
        if value.is_empty() {
            return Err(ReconcileError::new(
                "validate",
                "resolved secret must not be empty",
            ));
        }
        Ok(value)
    }
}

fn resolve_secrets<F>(
    local: &LocalDeployment,
    manifest_dir: &Path,
    get_env: &F,
) -> Result<ResolvedSecrets, ReconcileError>
where
    F: Fn(&str) -> Result<String, std::env::VarError>,
{
    Ok(ResolvedSecrets {
        operations_password: ResolvedSecrets::resolve(
            &local.secrets.operations_password,
            manifest_dir,
            get_env,
        )?,
        example_password: ResolvedSecrets::resolve(
            &local.secrets.example_password,
            manifest_dir,
            get_env,
        )?,
        dispatcher_password: ResolvedSecrets::resolve(
            &local.secrets.dispatcher_password,
            manifest_dir,
            get_env,
        )?,
        readiness_password: ResolvedSecrets::resolve(
            &local.secrets.readiness_password,
            manifest_dir,
            get_env,
        )?,
    })
}

fn validate_local_deployment(local: &LocalDeployment) -> Result<(), ReconcileError> {
    let valid_installation = !local.installation_id.is_empty()
        && local.installation_id.len() <= 40
        && local
            .installation_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    let valid_database = !local.database_name.is_empty()
        && local.database_name.len() <= 63
        && local
            .database_name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && local.database_name.as_bytes()[0].is_ascii_lowercase();
    if !valid_installation || !valid_database || local.http_port == 0 {
        return Err(ReconcileError::new(
            "validate",
            "local installation ID, database name, or HTTP port is invalid",
        ));
    }
    Ok(())
}

fn compose_project_name(installation_id: &str) -> String {
    let digest = sha2::Sha256::digest(installation_id.as_bytes());
    let suffix: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("bc-{}-{suffix}", installation_id.trim_matches('-'))
}

fn database_url(user: &str, password: &str, host: &str, database: &str) -> String {
    format!(
        "postgres://{}:{}@{host}:5432/{database}",
        user,
        utf8_percent_encode(password, NON_ALPHANUMERIC)
    )
}

struct ComposeContext {
    file: PathBuf,
    project: String,
    environment: BTreeMap<String, String>,
}

fn run_compose(
    context: &ComposeContext,
    phase: &'static str,
    args: &[&str],
) -> Result<(), ReconcileError> {
    let output: Output = Command::new("docker")
        .arg("compose")
        .arg("--project-name")
        .arg(&context.project)
        .arg("--file")
        .arg(&context.file)
        .args(args)
        .current_dir(context.file.parent().unwrap_or(Path::new(".")))
        .envs(&context.environment)
        .output()
        .map_err(|_| {
            ReconcileError::new(phase, format!("{DOCKER_COMPOSE} could not be started"))
        })?;
    if !output.status.success() {
        let status = output.status.code().map_or_else(
            || "terminated by signal".to_owned(),
            |code| format!("exit status {code}"),
        );
        let diagnostics =
            redacted_diagnostics(&output.stdout, &output.stderr, &context.environment);
        return Err(ReconcileError::new(
            phase,
            format!(
                "{DOCKER_COMPOSE} {} ({status}): {diagnostics}",
                args.join(" ")
            ),
        ));
    }
    Ok(())
}

fn redacted_diagnostics(
    stdout: &[u8],
    stderr: &[u8],
    environment: &BTreeMap<String, String>,
) -> String {
    let raw = format!(
        "{}{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );
    let mut sensitive_values: Vec<_> = environment
        .iter()
        .filter(|(key, value)| {
            !value.is_empty() && (key.ends_with("_PASSWORD") || key.ends_with("_DATABASE_URL"))
        })
        .map(|(_, value)| value.as_str())
        .collect();
    sensitive_values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    let redacted = sensitive_values
        .into_iter()
        .fold(raw, |message, secret| message.replace(secret, "[REDACTED]"));
    let message = redacted.trim();
    if message.is_empty() {
        return "Compose returned no diagnostic output".to_owned();
    }
    message.chars().take(1200).collect()
}

async fn verify_ready(url: &str) -> Result<(), ReconcileError> {
    let address: SocketAddr = url
        .strip_prefix("http://")
        .and_then(|value| value.split('/').next())
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| ReconcileError::new("readiness", "readiness address is invalid"))?;
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(Ok(status)) = timeout(Duration::from_secs(2), get_status(address)).await {
            if status == 200 {
                return Ok(());
            }
        }
        tokio::time::sleep(READY_INTERVAL).await;
    }
    Err(ReconcileError::new(
        "readiness",
        "API did not return HTTP 200 from /readyz before the 90 second timeout",
    ))
}

async fn get_status(address: SocketAddr) -> Result<u16, std::io::Error> {
    let mut stream = TcpStream::connect(address).await?;
    stream
        .write_all(b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let first_line = response
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    let status = std::str::from_utf8(first_line)
        .ok()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid HTTP status")
        })?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::{
        ReconcileRequest, ResolvedSecrets, compose_project_name, database_url,
        reconcile_with_environment,
    };
    use crate::manifest::{LocalSecrets, SecretReference};
    use std::path::Path;

    #[test]
    fn environment_secret_reference_resolves_without_debug_or_display_leakage() {
        let sentinel = "TOP_SECRET_SHOULD_NEVER_APPEAR_ENV";
        let value = ResolvedSecrets::resolve(
            &SecretReference::Environment {
                env: "SECRET".into(),
            },
            Path::new("."),
            &|_| Ok(sentinel.into()),
        )
        .unwrap();
        assert_eq!(value, sentinel);
        let error = super::ReconcileError::new(
            "validate",
            "required secret environment variable is unavailable",
        );
        assert!(!error.to_string().contains(sentinel));
        assert!(!format!("{error:?}").contains(sentinel));
    }

    #[test]
    fn file_secret_is_relative_to_manifest_and_trailing_newline_is_trimmed() {
        let dir = std::env::temp_dir().join(format!("bc-secret-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("secret"),
            "TOP_SECRET_SHOULD_NEVER_APPEAR_FILE\r\n",
        )
        .unwrap();
        let value = ResolvedSecrets::resolve(
            &SecretReference::File {
                file: "secret".into(),
            },
            &dir,
            &|_| unreachable!(),
        )
        .unwrap();
        assert_eq!(value, "TOP_SECRET_SHOULD_NEVER_APPEAR_FILE");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_and_empty_secrets_fail_with_redacted_errors() {
        for reference in [
            SecretReference::Environment {
                env: "MISSING".into(),
            },
            SecretReference::File {
                file: "not-found".into(),
            },
        ] {
            let error = ResolvedSecrets::resolve(&reference, Path::new("."), &|_| {
                Err(std::env::VarError::NotPresent)
            })
            .unwrap_err();
            assert!(!error.to_string().contains("TOP_SECRET"));
        }
        let error = ResolvedSecrets::resolve(
            &SecretReference::Environment {
                env: "EMPTY".into(),
            },
            Path::new("."),
            &|_| Ok(String::new()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("must not be empty"));
    }

    #[test]
    fn urls_and_project_names_use_credentials_safely_and_deterministically() {
        let url = database_url(
            "role",
            "TOP_SECRET_SHOULD_NEVER_APPEAR",
            "postgres",
            "bc_demo",
        );
        assert!(!url.contains("TOP_SECRET_SHOULD_NEVER_APPEAR"));
        assert_eq!(
            compose_project_name("demo-install"),
            compose_project_name("demo-install")
        );
        assert_ne!(
            compose_project_name("demo-install"),
            compose_project_name("another")
        );
    }

    #[test]
    fn typed_secret_reference_rejects_inline_plaintext() {
        let sentinel = "TOP_SECRET_SHOULD_NEVER_APPEAR_INLINE";
        let error = serde_yaml::from_str::<SecretReference>(sentinel).unwrap_err();
        assert!(!error.to_string().contains(sentinel));
        assert!(serde_yaml::from_str::<SecretReference>("env: KEY\nfile: both").is_err());
    }

    #[test]
    fn required_secret_config_has_only_secret_references() {
        let parsed: Result<LocalSecrets, _> = serde_yaml::from_str(
            "operations_password: { env: OPS }\nexample_password: { file: ./secrets/example }\ndispatcher_password: { env: DISPATCH }\nreadiness_password: { env: READY }",
        );
        assert!(parsed.is_ok());
    }

    #[tokio::test]
    async fn invalid_module_configuration_fails_before_secret_resolution_or_compose() {
        let dir = std::env::temp_dir().join(format!("bc-invalid-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("manifest.yaml");
        std::fs::write(
            &path,
            "release: 0.1.0\ndeployment_mode: self_hosted\nmodules:\n  example:\n    version: 0.1.0\n    configuration:\n      label: 42\nlocal:\n  installation_id: invalid\n  database_name: bc_invalid\n  http_port: 3901\n  secrets:\n    operations_password: { env: MISSING }\n    example_password: { env: MISSING }\n    dispatcher_password: { env: MISSING }\n    readiness_password: { env: MISSING }\n",
        ).unwrap();

        let result = reconcile_with_environment(ReconcileRequest { manifest_path: path }, |_| {
            panic!("invalid module configuration must fail before resolving secrets or applying Compose")
        }).await;
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("module 'example' configuration is invalid")
        );
        assert!(!error.to_string().contains("secret"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn compose_definition_contains_references_not_secret_values() {
        let compose = include_str!("../../deploy/compose.yaml");
        for sentinel in [
            "TOP_SECRET_SHOULD_NEVER_APPEAR_OPS",
            "TOP_SECRET_SHOULD_NEVER_APPEAR_MODULE",
        ] {
            assert!(!compose.contains(sentinel));
        }
        assert!(compose.contains("${OPERATIONS_DATABASE_URL:?operations URL is required}"));
    }

    #[test]
    fn compose_failure_diagnostics_are_actionable_and_redact_secrets_and_urls() {
        use std::collections::BTreeMap;

        let secret = "TOP_SECRET_SHOULD_NEVER_APPEAR_FAILURE";
        let url = format!("postgres://postgres:{secret}@postgres:5432/bc_demo");
        let stderr = format!("connection failed using {secret} and {url}").into_bytes();
        let environment = BTreeMap::from([
            ("BC_OPERATIONS_PASSWORD".to_owned(), secret.to_owned()),
            ("OPERATIONS_DATABASE_URL".to_owned(), url),
        ]);
        let diagnostics = super::redacted_diagnostics(&[], &stderr, &environment);
        assert!(diagnostics.contains("connection failed"));
        assert!(!diagnostics.contains(secret));
        assert!(!diagnostics.contains("postgres://"));
    }
}
