use opentelemetry::global;
use opentelemetry::KeyValue;
use opentelemetry_prometheus::exporter as otel_prom_exporter;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::Resource;
use prometheus::{Encoder, Registry, TextEncoder};
use crate::leslie::Leslie;
use std::net::SocketAddr;

use hyper::{Method, Request, Response, StatusCode};
use hyper::body::Incoming;
use http_body_util::Full;
use bytes::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

pub fn init_otel(node: &Leslie) -> Result<(), Box<dyn std::error::Error>> {
    let exporter = otel_prom_exporter().with_registry(node.registry.clone()).build()?;

    // Use node id in service.name so target_info is meaningful
    let svc_name = format!("leslie-{}", node.node_id);
    let resource = Resource::new([
            KeyValue::new("service.name", svc_name),
            KeyValue::new("service.instance.id", node.node_id.clone()),
        ]);

    let meter_provider = SdkMeterProvider::builder()
        .with_reader(exporter)
        .with_resource(resource)
        .build();
    global::set_meter_provider(meter_provider);

    // Metrics HTTP server (hyper) using a simple accept loop
    let addr: SocketAddr = std::env::var("PROMETHEUS_BIND")
        .unwrap_or_else(|_| "0.0.0.0:9464".to_string())
        .parse()?;

    let registry = node.registry.clone();
    tokio::spawn(async move {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                if let Ok(local) = listener.local_addr() {
                    tracing::info!("metrics listening on {}", local);
                }
                loop {
                    match listener.accept().await {
                        Ok((stream, _)) => {
                            let registry = registry.clone();
                            tokio::spawn(async move {
                                let svc = service_fn(move |req: Request<Incoming>| {
                                    let registry = registry.clone();
                                    async move {
                                        match (req.method(), req.uri().path()) {
                                            (&Method::GET, "/metrics") => {
                                                let metric_families = registry.gather();
                                                let mut buffer = Vec::new();
                                                let encoder = TextEncoder::new();
                                                if encoder.encode(&metric_families, &mut buffer).is_ok() {
                                                    let mut resp: Response<Full<Bytes>> = Response::new(Full::new(Bytes::from(buffer)));
                                                    *resp.status_mut() = StatusCode::OK;
                                                    resp.headers_mut().insert(
                                                        hyper::header::CONTENT_TYPE,
                                                        hyper::header::HeaderValue::from_str(encoder.format_type())
                                                            .unwrap_or(hyper::header::HeaderValue::from_static("text/plain")),
                                                    );
                                                    Ok::<_, hyper::Error>(resp)
                                                } else {
                                                    let mut resp: Response<Full<Bytes>> = Response::new(Full::new(Bytes::from_static(b"encode error")));
                                                    *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                                                    resp.headers_mut().insert(
                                                        hyper::header::CONTENT_TYPE,
                                                        hyper::header::HeaderValue::from_static("text/plain"),
                                                    );
                                                    Ok::<_, hyper::Error>(resp)
                                                }
                                            }
                                            _ => {
                                                let mut resp: Response<Full<Bytes>> = Response::new(Full::new(Bytes::from_static(b"not found")));
                                                *resp.status_mut() = StatusCode::NOT_FOUND;
                                                Ok::<_, hyper::Error>(resp)
                                            }
                                        }
                                    }
                                });
                                let io = TokioIo::new(stream);
                                if let Err(e) = http1::Builder::new().serve_connection(io, svc).await {
                                    tracing::error!("conn error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("accept error: {}", e);
                        }
                    }
                }
            }
            Err(e) => tracing::error!("metrics bind failed: {}", e),
        }
    });

    Ok(())
}
