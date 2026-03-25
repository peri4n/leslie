use ::log::info;
use std::io::Write;
use clap::Parser;
use std::sync::Arc;
use tokio::time::Duration;
use tonic::{Request, transport::Server};

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
    init_logging();

    let args = Args::parse();

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
        .serve(addr)
        .await?;
    Ok(())
}

fn init_logging() {
    let env = env_logger::Env::default().default_filter_or("info");
    let mut builder = env_logger::Builder::from_env(env);
    builder.format(|buf, record| {
        let ts = buf.timestamp_millis();
        let pid = std::process::id();
        writeln!(buf, "[{} {} pid={}] {}", ts, record.level(), pid, record.args())
    });
    builder.init();
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
