use http::Uri;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::time::{Duration, Interval, interval};
use tonic::Request;
use tracing::info;

use crate::services::clusterinfo::proto::ShareRequest;
use crate::services::clusterinfo::proto::cluster_info_client::ClusterInfoClient;
use crate::state::AppState;

pub struct ClusterInfoTask {
    state: Arc<AppState>,
    tick: Interval,
}

impl ClusterInfoTask {
    pub fn new(state: Arc<AppState>, every: Duration) -> Self {
        ClusterInfoTask {
            state,
            tick: interval(every),
        }
    }
}

impl Future for ClusterInfoTask {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        while self.tick.poll_tick(cx).is_ready() {
            let state = self.state.clone();

            // Skip gossip if node is "dead"
            if !state.is_alive() {
                continue;
            }

            let id = state.identity.node_id.clone();
            tokio::spawn(async move {
                let peer_addrs = state.cluster.list_addresses().await;
                if peer_addrs.is_empty() {
                    return;
                }
                for peer_addr in peer_addrs {
                    let target = if peer_addr.scheme().is_some() {
                        peer_addr.to_string()
                    } else {
                        format!("http://{}", peer_addr)
                    };
                    match ClusterInfoClient::connect(target.clone()).await {
                        Ok(mut c) => {
                            match c
                                .share(Request::new(ShareRequest {
                                    node_id: id.clone(),
                                    address: state.identity.public_uri.to_string(),
                                    peers: state.cluster.snapshot_peers().await.into_iter().collect(),
                                }))
                                .await
                            {
                                Ok(resp) => {
                                    let reply = resp.into_inner();
                                    for (pid, paddr) in reply.peers {
                                        if let Ok(uri) = paddr.parse::<Uri>() {
                                            state.cluster.add_peer(pid, uri).await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    info!("share {} failed: {}", peer_addr, e);
                                }
                            }
                        }
                        Err(e) => {
                            info!("connect {} failed: {}", peer_addr, e);
                        }
                    }
                }
            });
        }
        Poll::Pending
    }
}
