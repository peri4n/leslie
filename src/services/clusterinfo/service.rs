use std::collections::HashMap;

use http::Uri;
use tonic::{Request, Response, Status, async_trait};
use tracing::info;

use crate::{services::clusterinfo::proto::{DeregisterReply, DeregisterRequest, RegisterReply, RegisterRequest, ShareReply, ShareRequest, cluster_info_server::ClusterInfo}, state::AppState};


#[async_trait]
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
