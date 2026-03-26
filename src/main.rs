use clap::Parser;
use std::sync::Arc;
use tokio::time::Duration;
use tonic::{Request, transport::Server};
use tracing::info;
mod otel;
use crate::otel::init_metrics;

use crate::cli::Args;
use crate::gossiper::Gossiper;
use crate::leslie::Leslie;
use crate::leslie::gossip::GossipRequest;
use crate::leslie::gossip::gossip_client::GossipClient;
use crate::leslie::gossip::gossip_server::GossipServer;

pub mod cli;
pub mod gossiper;
pub mod leslie;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // init logs and metrics
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    init_metrics(&args.id)?;

    let leslie = Arc::new(Leslie::new(args.id.clone()));

    let addr = format!("{}:{}", args.hostname, args.port).parse()?;
    info!("Starting node {} on {:?}", leslie.node_id, addr);

    if let Some(connect_addr) = &args.connect {
        hello_to_seed_peer(
            connect_addr.clone(),
            args.id.clone(),
            format!("{}:{}", args.hostname.clone(), args.port),
        );
    }

    tokio::spawn(Gossiper::new(
        leslie.clone(),
        args.id.clone(),
        format!("{}:{}", args.hostname.clone(), args.port),
        Duration::from_secs(5),
    ));

    Server::builder()
        .add_service(GossipServer::from_arc(leslie))
        .serve_with_shutdown(addr, shutdown_signal())
        .await?;

    Ok(())
}

pub fn hello_to_seed_peer(seed_addr: String, self_id: String, self_addr: String) {
    tokio::spawn(async move {
        let target = format!("http://{}", seed_addr);
        match GossipClient::connect(target.clone()).await {
            Ok(mut c) => {
                match c
                    .gossip(Request::new(GossipRequest {
                        node_id: self_id.clone(),
                        address: self_addr.clone(),
                    }))
                    .await
                {
                    Ok(resp) => {
                        let reply = resp.into_inner();
                        info!(
                            "hello to seed peer {} success, got peers={:?}",
                            seed_addr, reply.peers
                        );
                    }
                    Err(e) => {
                        info!("gossip {} failed: {}", seed_addr, e);
                    }
                }
            }
            Err(e) => {
                info!("connect {} failed: {}", seed_addr, e);
            }
        }
    });
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("sigterm");
        let mut int = signal(SignalKind::interrupt()).expect("sigint");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
            _ = int.recv() => {},
        }
    }
    info!("shutdown signal received");
}
