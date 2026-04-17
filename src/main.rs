use clap::Parser;
use hyper::Uri;
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use tonic::{Request, transport::Server};
use tracing::info;
mod otel;
use crate::otel::init_otel;

use crate::chaos::{ChaosLayer, spawn_chaos_task};
use crate::cli::Args;
use crate::services::clusterinfo::proto::{DeregisterRequest, RegisterRequest};
use crate::services::clusterinfo::proto::cluster_info_client::ClusterInfoClient;
use crate::services::clusterinfo::proto::cluster_info_server::ClusterInfoServer;
use crate::services::clusterinfo::task::ClusterInfoTask;
use crate::state::{AppState, ClusterState, IdentityState, MetricsState};

pub mod chaos;
pub mod cli;
pub mod state;
pub mod services;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // init logs and metrics
    tracing_subscriber::fmt::init();

    // parse command line arguments
    let args = Args::parse();
    let bind_addr: SocketAddr = tokio::net::lookup_host(format!("{}:{}", args.hostname, args.port))
        .await?
        .next()
        .ok_or("failed to resolve hostname")?;

    // Public URI other peers should use to reach this leslie process
    let public_uri: Uri = format!("http://{}:{}", args.hostname, args.port).parse()?;
    let identity = Arc::new(IdentityState::new(
        args.id.clone(),
        public_uri.clone(),
        args.connect.clone(),
    ));

    let cluster = Arc::new(ClusterState::new());
    let metrics = Arc::new(MetricsState::new());
    let alive = Arc::new(AtomicBool::new(true));
    let rng = match args.rng_seed {
        Some(seed) => {
            info!("Using deterministic RNG with seed={}", seed);
            StdRng::seed_from_u64(seed)
        }
        None => StdRng::from_os_rng(),
    };

    let app_state = Arc::new(AppState {
        identity: identity.clone(),
        cluster: cluster.clone(),
        metrics: metrics.clone(),
        alive: alive.clone(),
        rng: Arc::new(Mutex::new(rng)),
    });

    init_otel(&app_state)?;

    if let Some(connect_addr) = &args.connect {
        info!("Registering to cluster at address={:?}", connect_addr);
        hello_to_seed_peer(
            connect_addr.clone(),
            identity.node_id.clone(),
            public_uri.clone(),
        );
    };

    tokio::spawn(ClusterInfoTask::new(app_state.clone(), Duration::from_secs(5)));

    // Spawn chaos background task (no-op when crash_probability == 0)
    spawn_chaos_task(
        app_state.clone(),
        args.crash_probability,
        Duration::from_secs(args.recovery_time),
    );

    let server_addr = format!("[::]:{}", args.port).parse()?;
    info!("Node {} started listening on {:?}", identity.node_id, server_addr);
    Server::builder()
        .layer(ChaosLayer::new(alive))
        .add_service(ClusterInfoServer::from_arc(app_state.clone()))
        .serve_with_shutdown(
            server_addr,
            shutdown_signal(identity.node_id.clone(), app_state.clone()),
        )
        .await?;

    Ok(())
}

pub fn hello_to_seed_peer(seed_uri: Uri, self_id: String, self_uri: Uri) {
    tokio::spawn(async move {
        // Accept either a bare host:port or a full URI.
        let target = if seed_uri.scheme_str() == Some("http") {
            seed_uri.to_string()
        } else {
            format!("http://{}", seed_uri)
        };

        match ClusterInfoClient::connect(target.clone()).await {
            Ok(mut c) => {
                match c
                    .register(Request::new(RegisterRequest {
                        node_id: self_id.clone(),
                        address: self_uri.to_string(),
                    }))
                    .await
                {
                    Ok(resp) => {
                        let reply = resp.into_inner();
                        info!(
                            "register with seed {} success, got peers={:?}",
                            target, reply.peers
                        );
                    }
                    Err(e) => {
                        info!("register {} failed: {}", target, e);
                    }
                }
            }
            Err(e) => {
                info!("connect {} failed: {}", target, e);
            }
        }
    });
}

async fn shutdown_signal(node_id: String, app: Arc<AppState>) {
    wait_for_shutdown().await;

    info!("shutdown signal received; deregistering");
    let peers = app.cluster.snapshot_peers().await;
    for uri in peers.values() {
        let target = uri.clone();
        if let Ok(mut c) = ClusterInfoClient::connect(target.clone()).await {
            let _ = c
                .deregister(Request::new(DeregisterRequest {
                    node_id: node_id.clone(),
                }))
                .await;
        }
    }
}

async fn wait_for_shutdown() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = sigterm.recv() => {},
    }
}
