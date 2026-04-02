use prometheus::Registry;
use rand::rngs::StdRng;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use http::Uri;
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
    // Bind address for the server
    pub public_uri: Uri,
    // Seed peer to register with on (re-)join
    pub seed_uri: Option<Uri>,
}

impl IdentityState {
    pub fn new(node_id: String, public_uri: Uri, seed_uri: Option<Uri>) -> Self {
        Self { node_id, public_uri, seed_uri }
    }
}

/// Cluster membership and peer data
#[derive(Debug, Clone)]
pub struct ClusterState {
    pub peers: Arc<RwLock<HashMap<String, Uri>>>,
}

impl ClusterState {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_peer(&self, node_id: String, address: Uri) {
        self.peers.write().await.insert(node_id, address);
    }
    pub async fn snapshot_peers(&self) -> HashMap<String, String> {
        let map: HashMap<String, Uri> = self.peers.read().await.clone();
        map.into_iter().map(|(id, uri)| (id, uri.to_string())).collect()
    }
    pub async fn list_addresses(&self) -> Vec<Uri> {
        let map: HashMap<String, Uri> = self.peers.read().await.clone();
        let mut set: HashSet<Uri> = HashSet::new();
        for addr in map.values() {
            set.insert(addr.clone());
        }
        set.into_iter().collect()
    }
    pub async fn clear_peers(&self) {
        self.peers.write().await.clear();
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
    pub alive: Arc<AtomicBool>,
    /// Shared deterministic RNG, seeded via --rng-seed
    pub rng: Arc<Mutex<StdRng>>,
}

impl AppState {
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
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
            let uri: Uri = incoming
                .address
                .parse()
                .map_err(|e| Status::invalid_argument(format!("Invalid URI: {}", e)))?;
            self.cluster.add_peer(incoming.node_id, uri).await;
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
        let uri: Uri = incoming
            .address
            .parse()
            .map_err(|e| Status::invalid_argument(format!("Invalid URI: {}", e)))?;
        self.cluster.add_peer(incoming.node_id, uri).await;
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
            let mut peers: tokio::sync::RwLockWriteGuard<'_, HashMap<String, Uri>> = self.cluster.peers.write().await;
            peers.remove(&incoming.node_id);
        }
        Ok(Response::new(DeregisterReply { ok: true }))
    }
}
