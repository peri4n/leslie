use std::future::Future;
use std::sync::Arc;
use crate::leslie::gossip::gossip_client::GossipClient;
use crate::leslie::gossip::GossipRequest;
use ::log::info;
use tonic::Request;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::time::{Duration, Interval, interval};

use crate::leslie::Leslie;

// Periodic reporter as a Future
pub struct Gossiper {
    leslie: Arc<Leslie>,
    self_id: String,
    self_addr: String,
    tick: Interval,
}

impl Gossiper {
    pub fn new(leslie: Arc<Leslie>, self_id: String, self_addr: String, every: Duration) -> Self {
        Gossiper {
            leslie,
            self_id,
            self_addr,
            tick: interval(every),
        }
    }
}

impl Future for Gossiper {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        while self.tick.poll_tick(cx).is_ready() {
            let leslie = self.leslie.clone();
            let id = self.self_id.clone();
            let self_addr = self.self_addr.clone();
            tokio::spawn(async move {
                let peer_addrs = leslie.list_addresses().await;
                if peer_addrs.is_empty() {
                    return;
                }
                info!("report tick peers={:?}", peer_addrs);
                for peer_addr in peer_addrs {
                    let target = format!("http://{}", peer_addr);
                    match GossipClient::connect(target.clone()).await {
                        Ok(mut c) => {
                            match c
                                .gossip(Request::new(GossipRequest {
                                    node_id: id.clone(),
                                    address: self_addr.clone(),
                                }))
                                .await
                            {
                                Ok(resp) => {
                                    let reply = resp.into_inner();
                                    for (pid, paddr) in reply.peers {
                                        let response_addr = paddr.parse().expect("Unable to parse addr");
                                        leslie.add_peer(pid, response_addr).await;
                                    }
                                }
                                Err(e) => {
                                    info!("gossip {} failed: {}", peer_addr, e);
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
