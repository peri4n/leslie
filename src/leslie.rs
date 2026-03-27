use std::{collections::{HashMap, HashSet}, net::SocketAddr, sync::Arc};
use prometheus::Registry;
use tokio::sync::RwLock;

use tonic::{Request, Response, Status};
use tracing::info;

use crate::leslie::gossip::{GossipReply, GossipRequest, gossip_server::Gossip};


pub mod gossip {
    tonic::include_proto!("gossip");
}

/// State of every Leslie node
#[derive(Debug, Clone)]
pub struct Leslie {
    /// id of this node
    pub node_id: String,

    /// peers of the node, mapping node_id to address
    pub peers: Arc<RwLock<HashMap<String, SocketAddr>>>,

    /// Prometheus registry used for metrics
    pub registry: Registry,
}

impl Leslie {
    pub fn new(node_id: String) -> Self {
        Leslie {
            node_id,
            peers: Arc::new(RwLock::new(HashMap::new())),
            registry: Registry::new(),
        }
    }
    pub async fn add_peer(&self, node_id: String, address: SocketAddr) {
        self.peers.write().await.insert(node_id, address);
    }
    pub async fn snapshot_peers(&self) -> HashMap<String, SocketAddr> {
        self.peers.read().await.clone()
    }
    pub async fn list_addresses(&self) -> Vec<SocketAddr> {
        let map = self.peers.read().await;
        let mut set: HashSet<SocketAddr> = HashSet::new();
        for addr in map.values() {
            set.insert(*addr);
        }
        set.into_iter().collect()
    }
}

#[tonic::async_trait]
impl Gossip for Leslie {
    async fn gossip(
        &self,
        request: Request<GossipRequest>,
    ) -> Result<Response<GossipReply>, Status> {
        // Simple metric: count gossip requests
        let meter = opentelemetry::global::meter("leslie");
        let counter = meter
            .u64_counter("gossip_requests_total")
            .with_description("total gossip RPCs")
            .init();
        counter.add(1, &[]);

        info!("Received gossip request: {:?}", request);
        let incoming = request.into_inner();
        if !incoming.node_id.is_empty() && !incoming.address.is_empty() {
            let addr = incoming
                .address
                .parse()
                .map_err(|e| Status::invalid_argument(format!("Invalid address: {}", e)))?;
            self.add_peer(incoming.node_id, addr).await;
        }
        let peers = self.snapshot_peers().await;
        let reply = GossipReply {
            peers: peers
                .into_iter()
                .map(|(id, addr)| (id, addr.to_string()))
                .collect(),
        };
        Ok(Response::new(reply))
    }
}
