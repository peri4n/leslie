use crate::state::AppState;
use opentelemetry::global;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_prometheus::exporter as otel_prom_exporter;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::Resource;
use prometheus::{Encoder, TextEncoder};
use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

/// Initialize the full OpenTelemetry stack: metrics (Prometheus) + traces (OTLP).
///
/// Also sets up the global `tracing` subscriber with:
///   - `EnvFilter` (respects `RUST_LOG`)
///   - `fmt` layer (human-readable stdout logs)
///   - `tracing-opentelemetry` layer (exports spans as OTel traces via OTLP)
///
/// Must be called **before** any `tracing` macros fire.
pub fn init_otel(state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let node_id = &state.identity.node_id;
    let svc_name = format!("leslie-{}", node_id);

    // Shared resource labels for both metrics and traces
    let resource = Resource::new([
        KeyValue::new("service.name", svc_name.clone()),
        KeyValue::new("service.instance.id", node_id.clone()),
    ]);

    // ── Metrics (Prometheus) ─────────────────────────────────────────────
    let prom_exporter = otel_prom_exporter()
        .with_registry(state.metrics.registry.clone())
        .build()?;

    let meter_provider = SdkMeterProvider::builder()
        .with_reader(prom_exporter)
        .with_resource(resource.clone())
        .build();
    global::set_meter_provider(meter_provider);

    // ── Traces (OTLP → Jaeger) ──────────────────────────────────────────
    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4317".to_string());

    let otlp_exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(&otlp_endpoint);

    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(otlp_exporter)
        .with_trace_config(
            opentelemetry_sdk::trace::config().with_resource(resource),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    // ── Tracing subscriber ──────────────────────────────────────────────
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .init();

    info!(
        endpoint = %otlp_endpoint,
        "OpenTelemetry tracing initialized"
    );

    // ── Prometheus HTTP server ───────────────────────────────────────────
    let addr: SocketAddr = std::env::var("PROMETHEUS_BIND")
        .unwrap_or_else(|_| "0.0.0.0:9464".to_string())
        .parse()?;

    let registry = state.metrics.registry.clone();
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
                                                if encoder
                                                    .encode(&metric_families, &mut buffer)
                                                    .is_ok()
                                                {
                                                    let mut resp: Response<Full<Bytes>> =
                                                        Response::new(Full::new(Bytes::from(
                                                            buffer,
                                                        )));
                                                    *resp.status_mut() = StatusCode::OK;
                                                    resp.headers_mut().insert(
                                                        hyper::header::CONTENT_TYPE,
                                                        hyper::header::HeaderValue::from_str(
                                                            encoder.format_type(),
                                                        )
                                                        .unwrap_or(
                                                            hyper::header::HeaderValue::from_static(
                                                                "text/plain",
                                                            ),
                                                        ),
                                                    );
                                                    Ok::<_, hyper::Error>(resp)
                                                } else {
                                                    let mut resp: Response<Full<Bytes>> =
                                                        Response::new(Full::new(
                                                            Bytes::from_static(b"encode error"),
                                                        ));
                                                    *resp.status_mut() =
                                                        StatusCode::INTERNAL_SERVER_ERROR;
                                                    resp.headers_mut().insert(
                                                        hyper::header::CONTENT_TYPE,
                                                        hyper::header::HeaderValue::from_static(
                                                            "text/plain",
                                                        ),
                                                    );
                                                    Ok::<_, hyper::Error>(resp)
                                                }
                                            }
                                            _ => {
                                                let mut resp: Response<Full<Bytes>> = Response::new(
                                                    Full::new(Bytes::from_static(b"not found")),
                                                );
                                                *resp.status_mut() = StatusCode::NOT_FOUND;
                                                Ok::<_, hyper::Error>(resp)
                                            }
                                        }
                                    }
                                });
                                let io = TokioIo::new(stream);
                                if let Err(e) =
                                    http1::Builder::new().serve_connection(io, svc).await
                                {
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
