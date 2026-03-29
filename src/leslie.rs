use prometheus::Registry;
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
};
use tokio::sync::RwLock;

use tonic::{Request, Response, Status};
use tracing::info;

use crate::leslie::clusterinfo::{
    DeregisterReply, DeregisterRequest, RegisterReply, RegisterRequest, ShareReply, ShareRequest,
    cluster_info_server::ClusterInfo,
};

pub mod clusterinfo {
    tonic::include_proto!("clusterinfo");
}

/// Identity (immutable) state
#[derive(Debug, Clone)]
pub struct IdentityState {
    pub node_id: String,
    pub address: SocketAddr,
}

impl IdentityState {
    pub fn new(node_id: String, address: SocketAddr) -> Self {
        Self { node_id, address }
    }
}

/// Cluster membership and peer data
#[derive(Debug, Clone)]
pub struct ClusterState {
    pub peers: Arc<RwLock<HashMap<String, SocketAddr>>>,
}

impl ClusterState {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_peer(&self, node_id: String, address: SocketAddr) {
        self.peers.write().await.insert(node_id, address);
    }
    pub async fn snapshot_peers(&self) -> HashMap<String, String> {
        self.peers
            .read()
            .await
            .clone()
            .into_iter()
            .map(|(id, addr)| (id, addr.to_string()))
            .collect()
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

/// Metrics/observability state
#[derive(Debug, Clone)]
pub struct MetricsState {
    pub registry: Registry,
}

impl MetricsState {
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
        }
    }
}

/// Top-level application state holding Arcs to sub-states
#[derive(Debug, Clone)]
pub struct AppState {
    pub identity: Arc<IdentityState>,
    pub cluster: Arc<ClusterState>,
    pub metrics: Arc<MetricsState>,
}

#[tonic::async_trait]
impl ClusterInfo for AppState {
    async fn share(&self, request: Request<ShareRequest>) -> Result<Response<ShareReply>, Status> {
        // Simple metric: count gossip requests
        let meter = opentelemetry::global::meter("leslie");
        let counter = meter
            .u64_counter("gossip_requests_total")
            .with_description("total gossip RPCs")
            .init();
        counter.add(1, &[]);

        info!("Received share request: {:?}", request);
        let incoming = request.into_inner();
        if !incoming.node_id.is_empty() && !incoming.address.is_empty() {
            let addr = incoming
                .address
                .parse()
                .map_err(|e| Status::invalid_argument(format!("Invalid address: {}", e)))?;
            self.cluster.add_peer(incoming.node_id, addr).await;
        }
        let reply = ShareReply {
            peers: self.cluster.snapshot_peers().await,
        };
        Ok(Response::new(reply))
    }

    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterReply>, Status> {
        let incoming = request.into_inner();
        let addr = incoming
            .address
            .parse()
            .map_err(|e| Status::invalid_argument(format!("Invalid address: {}", e)))?;
        self.cluster.add_peer(incoming.node_id, addr).await;
        let reply = RegisterReply {
            peers: self.cluster.snapshot_peers().await,
        };
        Ok(Response::new(reply))
    }

    async fn deregister(
        &self,
        request: Request<DeregisterRequest>,
    ) -> Result<Response<DeregisterReply>, Status> {
        let incoming = request.into_inner();
        {
            let mut peers = self.cluster.peers.write().await;
            peers.remove(&incoming.node_id);
        }
        Ok(Response::new(DeregisterReply { ok: true }))
    }
}
