use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use rand::Rng;
use tokio::time::{Duration, sleep};
use tower_layer::Layer;
use tower_service::Service;
use tracing::info;

use crate::hello_to_seed_peer;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Tower layer: simulate timeout when node is "dead"
// ---------------------------------------------------------------------------

/// Tower layer that wraps services with a chaos check.
#[derive(Clone)]
pub struct ChaosLayer {
    alive: Arc<AtomicBool>,
}

impl ChaosLayer {
    pub fn new(alive: Arc<AtomicBool>) -> Self {
        Self { alive }
    }
}

impl<S> Layer<S> for ChaosLayer {
    type Service = ChaosService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        ChaosService {
            inner,
            alive: self.alive.clone(),
        }
    }
}

/// Wrapper service that stalls requests when the node is dead.
#[derive(Clone)]
pub struct ChaosService<S> {
    inner: S,
    alive: Arc<AtomicBool>,
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

impl<S, ReqBody> Service<http::Request<ReqBody>> for ChaosService<S>
where
    S: Service<
            http::Request<ReqBody>,
            Response = http::Response<tonic::body::Body>,
            Error = Infallible,
        > + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        if !self.alive.load(Ordering::Relaxed) {
            // Node is "dead": sleep 30s to simulate a timeout, then return 503.
            // In practice the caller's gRPC deadline fires first.
            Box::pin(async move {
                sleep(Duration::from_secs(30)).await;
                let resp = http::Response::builder()
                    .status(503)
                    .body(tonic::body::Body::default())
                    .unwrap();
                Ok(resp)
            })
        } else {
            let mut svc = self.inner.clone();
            Box::pin(async move { svc.call(req).await })
        }
    }
}

// ---------------------------------------------------------------------------
// Background chaos task
// ---------------------------------------------------------------------------

/// Spawns a background loop that randomly crashes and recovers the node.
pub fn spawn_chaos_task(
    state: Arc<AppState>,
    crash_probability: f64,
    recovery_time: Duration,
) {
    if crash_probability <= 0.0 {
        return; // chaos disabled
    }

    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(1)).await;

            if !state.is_alive() {
                continue; // already dead, recovery timer handles wake-up
            }

            // Use the shared deterministic RNG (lock is sync-only, never
            // held across an await point).
            let roll: f64 = {
                let mut rng = state.rng.lock().unwrap();
                rng.random()
            };
            if roll < crash_probability {
                // --- simulate crash ---
                info!(
                    "CHAOS: node {} simulating crash (roll={:.4} < {:.4})",
                    state.identity.node_id, roll, crash_probability
                );
                state.alive.store(false, Ordering::Relaxed);
                state.cluster.clear_peers().await;

                // wait recovery_time then come back
                sleep(recovery_time).await;

                info!(
                    "CHAOS: node {} recovering after {}s",
                    state.identity.node_id,
                    recovery_time.as_secs()
                );
                state.alive.store(true, Ordering::Relaxed);

                // re-register with seed if configured
                if let Some(seed) = &state.identity.seed_uri {
                    hello_to_seed_peer(
                        seed.clone(),
                        state.identity.node_id.clone(),
                        state.identity.public_uri.clone(),
                    );
                }
            }
        }
    });
}
