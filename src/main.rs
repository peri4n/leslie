use clap::Parser;
use std::sync::Arc;
use tokio::time::Duration;
use tonic::{Request, transport::Server};
use tracing::info;
mod otel;
use crate::otel::init_otel;

use crate::cli::Args;
use crate::gossiper::Gossiper;
use crate::leslie::clusterinfo::DeregisterRequest;
use crate::leslie::clusterinfo::RegisterRequest;
use crate::leslie::clusterinfo::cluster_info_client::ClusterInfoClient;
use crate::leslie::clusterinfo::cluster_info_server::ClusterInfoServer;
use crate::leslie::{AppState, ClusterState, IdentityState, MetricsState};

pub mod cli;
pub mod gossiper;
pub mod leslie;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // init logs and metrics
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let addr = format!("{}:{}", args.hostname, args.port).parse()?;
    let identity = Arc::new(IdentityState::new(args.id.clone(), addr));
    let cluster = Arc::new(ClusterState::new());
    let metrics = Arc::new(MetricsState::new());
    let app_state = Arc::new(AppState {
        identity: identity.clone(),
        cluster: cluster.clone(),
        metrics: metrics.clone(),
    });

    init_otel(&app_state)?;

    let addr = format!("{}:{}", args.hostname, args.port).parse()?;
    info!("Starting node {} on {:?}", identity.node_id, addr);

    if let Some(connect_addr) = &args.connect {
        hello_to_seed_peer(
            connect_addr.clone(),
            args.id.clone(),
            format!("{}:{}", args.hostname.clone(), args.port),
        );
    }

    tokio::spawn(Gossiper::new(app_state.clone(), Duration::from_secs(5)));

    Server::builder()
        .add_service(ClusterInfoServer::from_arc(app_state.clone()))
        .serve_with_shutdown(
            addr,
            shutdown_signal(identity.node_id.clone(), app_state.clone()),
        )
        .await?;

    Ok(())
}

pub fn hello_to_seed_peer(seed_addr: String, self_id: String, self_addr: String) {
    tokio::spawn(async move {
        let target = format!("http://{}", seed_addr);
        match ClusterInfoClient::connect(target.clone()).await {
            Ok(mut c) => {
                match c
                    .register(Request::new(RegisterRequest {
                        node_id: self_id.clone(),
                        address: self_addr.clone(),
                    }))
                    .await
                {
                    Ok(resp) => {
                        let reply = resp.into_inner();
                        info!(
                            "register with seed {} success, got peers={:?}",
                            seed_addr, reply.peers
                        );
                    }
                    Err(e) => {
                        info!("register {} failed: {}", seed_addr, e);
                    }
                }
            }
            Err(e) => {
                info!("connect {} failed: {}", seed_addr, e);
            }
        }
    });
}

async fn shutdown_signal(node_id: String, app: Arc<AppState>) {
    info!("shutdown signal received; deregistering");
    // Attempt deregister with all known peers
    let peers = app.cluster.snapshot_peers().await;
    for addr in peers.values() {
        let target = format!("http://{}", addr);
        if let Ok(mut c) = ClusterInfoClient::connect(target.clone()).await {
            let _ = c
                .deregister(Request::new(DeregisterRequest {
                    node_id: node_id.clone(),
                }))
                .await;
        }
    }
}
