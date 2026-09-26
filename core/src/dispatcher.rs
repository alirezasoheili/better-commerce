//! Shared PostgreSQL outbox dispatch infrastructure.
//!
//! This layer uses the scoped dispatcher pool and intentionally knows only the
//! module-owned outbox envelope, not any transport or broker representation.

use std::{
    collections::HashSet,
    future::Future,
    sync::{Arc, Mutex},
};

use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryRequest {
    pub event_id: String,
    pub event_type: String,
    pub event_version: i16,
    pub aggregate_type: String,
    pub aggregate_id: i64,
    pub aggregate_sequence: i64,
    pub occurred_at: String,
    pub payload_schema_version: i16,
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryFailure(pub String);

impl std::fmt::Display for DeliveryFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DeliveryFailure {}

/// Acknowledges acceptance for publication, not downstream business processing.
pub trait EventDelivery: Send + Sync {
    fn deliver(
        &self,
        event: DeliveryRequest,
    ) -> impl Future<Output = Result<(), DeliveryFailure>> + Send;
}

/// Idempotent M0 delivery adapter useful for in-process and integration tests.
#[derive(Clone, Default)]
pub struct InProcessEventDelivery {
    state: Arc<Mutex<InProcessState>>,
}

#[derive(Default)]
struct InProcessState {
    accepted_ids: HashSet<String>,
    attempts: Vec<String>,
    failures_remaining: usize,
}

impl InProcessEventDelivery {
    pub fn reject_next(&self, count: usize) {
        self.state
            .lock()
            .expect("delivery state mutex poisoned")
            .failures_remaining += count;
    }

    pub fn accepted_event_ids(&self) -> HashSet<String> {
        self.state
            .lock()
            .expect("delivery state mutex poisoned")
            .accepted_ids
            .clone()
    }

    pub fn attempts_for(&self, event_id: &str) -> usize {
        self.state
            .lock()
            .expect("delivery state mutex poisoned")
            .attempts
            .iter()
            .filter(|attempted| attempted.as_str() == event_id)
            .count()
    }
}

impl EventDelivery for InProcessEventDelivery {
    #[allow(clippy::manual_async_fn)] // The port guarantees its delivery future is Send.
    fn deliver(
        &self,
        event: DeliveryRequest,
    ) -> impl Future<Output = Result<(), DeliveryFailure>> + Send {
        async move {
            let mut state = self.state.lock().expect("delivery state mutex poisoned");
            state.attempts.push(event.event_id.clone());
            if state.failures_remaining > 0 {
                state.failures_remaining -= 1;
                return Err(DeliveryFailure("deliberate in-process rejection".into()));
            }
            state.accepted_ids.insert(event.event_id);
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimedEvent {
    pub request: DeliveryRequest,
    pub claimed_by: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchOutcome {
    NoEligibleEvent,
    Delivered { event_id: String },
    Rejected { event_id: String, reason: String },
    LeaseLost { event_id: String },
}

#[derive(Clone, Debug)]
pub struct PostgresOutboxDispatcher {
    pool: PgPool,
    lease_seconds: i64,
}

type ClaimedOutboxRow = (
    String,
    String,
    i16,
    String,
    i64,
    i64,
    String,
    i16,
    Value,
    String,
);

impl PostgresOutboxDispatcher {
    /// Build from the dedicated dispatcher database URL. This pool never uses
    /// the module runtime or operations identity.
    pub async fn connect(database_url: &str, lease_seconds: i64) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET search_path TO bc_example, pg_catalog")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(database_url)
            .await?;
        Ok(Self::with_pool(pool, lease_seconds))
    }

    pub fn with_pool(pool: PgPool, lease_seconds: i64) -> Self {
        assert!(lease_seconds > 0, "outbox lease must be positive");
        Self {
            pool,
            lease_seconds,
        }
    }

    /// Locks and leases the eligible aggregate head in one statement. Earlier
    /// unresolved rows are checked in PostgreSQL, so local process state cannot
    /// allow an aggregate to overtake its head.
    pub async fn claim_next(&self, worker_id: &str) -> Result<Option<ClaimedEvent>, sqlx::Error> {
        let row: Option<ClaimedOutboxRow> = sqlx::query_as(
            "WITH candidate AS (\
                 SELECT event_id FROM bc_example.outbox_events AS event \
                 WHERE event.delivered_at IS NULL \
                   AND event.available_at <= clock_timestamp() \
                   AND (event.claimed_by IS NULL OR event.claim_until <= clock_timestamp()) \
                   AND NOT EXISTS (\
                       SELECT 1 FROM bc_example.outbox_events AS earlier \
                       WHERE earlier.aggregate_type = event.aggregate_type \
                         AND earlier.aggregate_id = event.aggregate_id \
                         AND earlier.aggregate_sequence < event.aggregate_sequence \
                         AND earlier.delivered_at IS NULL \
                   ) \
                 ORDER BY event.occurred_at, event.event_id \
                 FOR UPDATE OF event SKIP LOCKED LIMIT 1\
             ), claimed AS (\
                 UPDATE bc_example.outbox_events AS event \
                 SET claimed_by = $1 || ':' || gen_random_uuid()::text, \
                     claim_until = clock_timestamp() + ($2 * interval '1 second'), \
                     attempt_count = event.attempt_count + 1 \
                 FROM candidate \
                 WHERE event.event_id = candidate.event_id \
                 RETURNING event.event_id::text, event.event_type, event.event_version, \
                     event.aggregate_type, event.aggregate_id, event.aggregate_sequence, \
                     event.occurred_at::text, event.payload_schema_version, event.payload, \
                     event.claimed_by\
             ) \
             SELECT event_id, event_type, event_version, aggregate_type, aggregate_id, \
                    aggregate_sequence, occurred_at, payload_schema_version, payload, claimed_by \
             FROM claimed",
        )
        .bind(worker_id)
        .bind(self.lease_seconds)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| ClaimedEvent {
            request: DeliveryRequest {
                event_id: row.0,
                event_type: row.1,
                event_version: row.2,
                aggregate_type: row.3,
                aggregate_id: row.4,
                aggregate_sequence: row.5,
                occurred_at: row.6,
                payload_schema_version: row.7,
                payload: row.8,
            },
            claimed_by: row.9,
        }))
    }

    /// Persists acceptance only after the caller has received EventDelivery's
    /// successful response. A stale lease owner cannot acknowledge another
    /// worker's claim.
    pub async fn acknowledge(&self, claim: &ClaimedEvent) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE bc_example.outbox_events \
             SET delivered_at = clock_timestamp(), claimed_by = NULL, claim_until = NULL \
             WHERE event_id = $1::uuid AND delivered_at IS NULL \
               AND claimed_by = $2 AND claim_until > clock_timestamp()",
        )
        .bind(&claim.request.event_id)
        .bind(&claim.claimed_by)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Releases a deliberately rejected attempt for immediate retry.
    pub async fn release_rejected(&self, claim: &ClaimedEvent) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE bc_example.outbox_events \
             SET claimed_by = NULL, claim_until = NULL, available_at = clock_timestamp() \
             WHERE event_id = $1::uuid AND delivered_at IS NULL AND claimed_by = $2",
        )
        .bind(&claim.request.event_id)
        .bind(&claim.claimed_by)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn dispatch_one<D: EventDelivery>(
        &self,
        worker_id: &str,
        delivery: &D,
    ) -> Result<DispatchOutcome, sqlx::Error> {
        let Some(claim) = self.claim_next(worker_id).await? else {
            return Ok(DispatchOutcome::NoEligibleEvent);
        };
        match delivery.deliver(claim.request.clone()).await {
            Ok(()) if self.acknowledge(&claim).await? => Ok(DispatchOutcome::Delivered {
                event_id: claim.request.event_id,
            }),
            Ok(()) => Ok(DispatchOutcome::LeaseLost {
                event_id: claim.request.event_id,
            }),
            Err(error) => {
                self.release_rejected(&claim).await?;
                Ok(DispatchOutcome::Rejected {
                    event_id: claim.request.event_id,
                    reason: error.0,
                })
            }
        }
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
