use better_commerce_example::database::{MIGRATOR as EXAMPLE_MIGRATOR, SCHEMA};
use percent_encoding::percent_decode_str;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use url::Url;

pub static SHARED_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/shared");
const SHARED_SCHEMA: &str = "bc_shared";

/// Runs under a separately provisioned operations identity. Runtime URLs are never used here.
pub async fn migrate_installation(
    operations_url: &str,
    example_enabled: bool,
    example_runtime_url: Option<&str>,
    dispatcher_url: Option<&str>,
    readiness_url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let operations = Url::parse(operations_url)?;
    let database_name =
        percent_decode_str(operations.path().trim_start_matches('/')).decode_utf8()?;
    let (expected_example_role, expected_dispatcher_role, expected_ready_role) =
        scoped_role_names(&database_name);
    let example_identity = if example_enabled {
        Some(RuntimeIdentity::parse(
            example_runtime_url.ok_or("enabled example module needs a runtime database URL")?,
            operations_url,
            &expected_example_role,
        )?)
    } else {
        None
    };
    let dispatcher_identity = if example_enabled {
        Some(RuntimeIdentity::parse(
            dispatcher_url.ok_or("enabled example module needs a dispatcher database URL")?,
            operations_url,
            &expected_dispatcher_role,
        )?)
    } else {
        None
    };
    let ready_identity =
        RuntimeIdentity::parse(readiness_url, operations_url, &expected_ready_role)?;
    let identities = [
        example_identity.as_ref(),
        dispatcher_identity.as_ref(),
        Some(&ready_identity),
    ];
    for left in 0..identities.len() {
        for right in (left + 1)..identities.len() {
            if identities[left]
                .is_some_and(|left| identities[right].is_some_and(|right| left.role == right.role))
            {
                return Err("runtime, dispatcher, and readiness must use distinct roles".into());
            }
        }
    }
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(operations_url)
        .await?;
    let mut connection = pool.acquire().await?;

    provision_role(&mut connection, &ready_identity).await?;
    if let Some(example_identity) = &example_identity {
        provision_role(&mut connection, example_identity).await?;
    }
    if let Some(dispatcher_identity) = &dispatcher_identity {
        provision_role(&mut connection, dispatcher_identity).await?;
    }
    revoke_database_public_connect(&mut connection).await?;
    sqlx::query("REVOKE ALL ON SCHEMA public FROM PUBLIC")
        .execute(&mut *connection)
        .await?;
    grant_database_connect(&mut connection, &ready_identity.role).await?;
    if let Some(example_identity) = &example_identity {
        grant_database_connect(&mut connection, &example_identity.role).await?;
    }
    if let Some(dispatcher_identity) = &dispatcher_identity {
        grant_database_connect(&mut connection, &dispatcher_identity.role).await?;
    }

    sqlx::query("CREATE SCHEMA IF NOT EXISTS bc_shared")
        .execute(&mut *connection)
        .await?;
    sqlx::query("SET search_path TO bc_shared, pg_catalog")
        .execute(&mut *connection)
        .await?;
    SHARED_MIGRATOR.run(&mut *connection).await?;
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA bc_shared TO {}",
        ready_identity.quoted_role()
    ))
    .execute(&mut *connection)
    .await?;
    sqlx::query(&format!(
        "GRANT SELECT ON TABLE bc_shared._sqlx_migrations TO {}",
        ready_identity.quoted_role()
    ))
    .execute(&mut *connection)
    .await?;

    if let Some(example_identity) = &example_identity {
        sqlx::query("CREATE SCHEMA IF NOT EXISTS bc_example")
            .execute(&mut *connection)
            .await?;
        sqlx::query("SET search_path TO bc_example, pg_catalog")
            .execute(&mut *connection)
            .await?;
        EXAMPLE_MIGRATOR.run(&mut *connection).await?;
        sqlx::query(&format!(
            "GRANT USAGE ON SCHEMA {SCHEMA} TO {}",
            example_identity.quoted_role()
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA {SCHEMA} TO {}",
            example_identity.quoted_role()
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            "GRANT USAGE ON ALL SEQUENCES IN SCHEMA {SCHEMA} TO {}",
            example_identity.quoted_role()
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            "REVOKE INSERT, UPDATE, DELETE ON TABLE {SCHEMA}._sqlx_migrations FROM {}",
            example_identity.quoted_role()
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA {SCHEMA} GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {}", example_identity.quoted_role()
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA {SCHEMA} GRANT USAGE ON SEQUENCES TO {}",
            example_identity.quoted_role()
        ))
        .execute(&mut *connection)
        .await?;
    }

    if let Some(dispatcher_identity) = &dispatcher_identity {
        let role = dispatcher_identity.quoted_role();
        sqlx::query(&format!("GRANT USAGE ON SCHEMA {SCHEMA} TO {role}"))
            .execute(&mut *connection)
            .await?;
        sqlx::query(&format!(
            "REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA {SCHEMA} FROM {role}"
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            "GRANT SELECT, UPDATE ON TABLE {SCHEMA}.outbox_events TO {role}"
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            "REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA {SCHEMA} FROM {role}"
        ))
        .execute(&mut *connection)
        .await?;
    }

    ensure_role_is_scoped(&mut connection, &ready_identity.role, SHARED_SCHEMA).await?;
    if let Some(example_identity) = &example_identity {
        ensure_role_is_scoped(&mut connection, &example_identity.role, SCHEMA).await?;
    }
    if let Some(dispatcher_identity) = &dispatcher_identity {
        ensure_role_is_scoped(&mut connection, &dispatcher_identity.role, SCHEMA).await?;
    }

    connection.close().await?;
    pool.close().await;
    Ok(())
}

/// Stable, database-specific login names for one installation.
pub fn scoped_role_names(database_name: &str) -> (String, String, String) {
    let slug: String = database_name
        .chars()
        .take(12)
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let digest = Sha256::digest(database_name.as_bytes());
    let fingerprint: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    (
        format!("bc_example_{slug}_{fingerprint}"),
        format!("bc_dispatch_{slug}_{fingerprint}"),
        format!("bc_ready_{slug}_{fingerprint}"),
    )
}

fn quoted_role(role: &str) -> Result<String, &'static str> {
    if role.is_empty()
        || !role
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("database role name must contain only ASCII letters, digits, or underscores");
    }
    Ok(format!("\"{role}\""))
}

struct RuntimeIdentity {
    role: String,
    password: String,
}

impl RuntimeIdentity {
    fn parse(
        runtime_url: &str,
        operations_url: &str,
        expected_role: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let runtime = Url::parse(runtime_url)?;
        let operations = Url::parse(operations_url)?;
        if runtime.host_str() != operations.host_str()
            || runtime.port_or_known_default() != operations.port_or_known_default()
            || runtime.path() != operations.path()
        {
            return Err(
                "runtime and operations URLs must target the same installation database".into(),
            );
        }
        let role = percent_decode_str(runtime.username())
            .decode_utf8()?
            .to_string();
        quoted_role(&role)?;
        if role != expected_role {
            return Err("runtime role name does not belong to this installation database".into());
        }
        let password =
            percent_decode_str(runtime.password().ok_or("runtime URL needs a password")?)
                .decode_utf8()?
                .to_string();
        if password.is_empty() {
            return Err("runtime URL needs a password".into());
        }
        let operations_role = percent_decode_str(operations.username()).decode_utf8()?;
        if role == operations_role {
            return Err("runtime and operations must use distinct roles".into());
        }
        Ok(Self { role, password })
    }

    fn quoted_role(&self) -> String {
        quoted_role(&self.role).expect("role was validated on parsing")
    }
}

async fn provision_role(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    identity: &RuntimeIdentity,
) -> Result<(), Box<dyn std::error::Error>> {
    let exists: (bool,) =
        sqlx::query_as("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
            .bind(&identity.role)
            .fetch_one(&mut **connection)
            .await?;
    let role = identity.quoted_role();
    let password = identity.password.replace('\'', "''");
    let statement = if exists.0 {
        let other_installation_grants: (i64,) = sqlx::query_as(
            "SELECT count(*) FROM pg_database AS database LEFT JOIN LATERAL aclexplode(database.datacl) AS grant_entry ON true WHERE database.datname <> current_database() AND (database.datdba = (SELECT oid FROM pg_roles WHERE rolname = $1) OR (grant_entry.grantee = (SELECT oid FROM pg_roles WHERE rolname = $1) AND grant_entry.privilege_type = 'CONNECT'))",
        )
        .bind(&identity.role)
        .fetch_one(&mut **connection)
        .await?;
        if other_installation_grants.0 != 0 {
            return Err("runtime role is already assigned to another installation database".into());
        }
        format!("ALTER ROLE {role} PASSWORD '{password}'")
    } else {
        format!("CREATE ROLE {role} LOGIN NOINHERIT PASSWORD '{password}'")
    };
    sqlx::query(&statement).execute(&mut **connection).await?;
    Ok(())
}

async fn grant_database_connect(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    role: &str,
) -> Result<(), sqlx::Error> {
    let database: (String,) = sqlx::query_as("SELECT current_database()")
        .fetch_one(&mut **connection)
        .await?;
    let statement = format!(
        "GRANT CONNECT ON DATABASE \"{}\" TO {}",
        database.0.replace('"', "\"\""),
        quoted_role(role).expect("role validated")
    );
    sqlx::query(&statement).execute(&mut **connection).await?;
    Ok(())
}

async fn revoke_database_public_connect(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
) -> Result<(), sqlx::Error> {
    let database: (String,) = sqlx::query_as("SELECT current_database()")
        .fetch_one(&mut **connection)
        .await?;
    let statement = format!(
        "REVOKE CONNECT ON DATABASE \"{}\" FROM PUBLIC",
        database.0.replace('"', "\"\"")
    );
    sqlx::query(&statement).execute(&mut **connection).await?;
    Ok(())
}

async fn ensure_role_is_scoped(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    role: &str,
    schema: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let privileged: (bool,) = sqlx::query_as(
        "SELECT NOT rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls OR rolinherit FROM pg_roles WHERE rolname = $1",
    )
    .bind(role)
    .fetch_one(&mut **connection)
    .await?;
    let memberships: (i64,) =
        sqlx::query_as("SELECT count(*) FROM pg_auth_members WHERE member = (SELECT oid FROM pg_roles WHERE rolname = $1)")
            .bind(role)
            .fetch_one(&mut **connection)
            .await?;
    let other_schemas: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM pg_namespace WHERE nspname <> $2 AND nspname NOT LIKE 'pg_%' AND nspname <> 'information_schema' AND (has_schema_privilege($1, oid, 'USAGE') OR has_schema_privilege($1, oid, 'CREATE'))",
    )
    .bind(role)
    .bind(schema)
    .fetch_one(&mut **connection)
    .await?;
    let database_create: (bool,) =
        sqlx::query_as("SELECT has_database_privilege($1, current_database(), 'CREATE')")
            .bind(role)
            .fetch_one(&mut **connection)
            .await?;
    if privileged.0 || memberships.0 != 0 || other_schemas.0 != 0 || database_create.0 {
        return Err("runtime database role has privileges outside its scoped schema".into());
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct ReadinessDatabase {
    shared: PgPool,
}

impl ReadinessDatabase {
    pub async fn connect(shared_readiness_url: &str) -> Result<Self, sqlx::Error> {
        let shared = PgPoolOptions::new()
            .max_connections(2)
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET search_path TO bc_shared, pg_catalog")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_lazy(shared_readiness_url)?;
        Ok(Self { shared })
    }

    pub async fn is_ready(&self) -> bool {
        let shared_rows: Result<Vec<(i64, bool, Vec<u8>)>, _> = sqlx::query_as(
            "SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(&self.shared)
        .await;
        let Ok(shared_rows) = shared_rows else {
            return false;
        };
        if shared_rows.len() != SHARED_MIGRATOR.migrations.len()
            || !shared_rows
                .iter()
                .zip(SHARED_MIGRATOR.migrations.iter())
                .all(|((version, success, checksum), expected)| {
                    *version == expected.version
                        && *success
                        && checksum.as_slice() == expected.checksum.as_ref()
                })
        {
            return false;
        }
        if sqlx::query("SELECT 1").execute(&self.shared).await.is_err() {
            return false;
        }
        true
    }

    pub async fn close(&self) {
        self.shared.close().await;
    }
}
