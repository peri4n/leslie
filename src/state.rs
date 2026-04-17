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

