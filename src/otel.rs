use opentelemetry::global;
use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use std::net::SocketAddr;
use axum::{routing::get, Router};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::body::Body;
use opentelemetry_prometheus::exporter as otel_prom_exporter;
use prometheus::{Encoder, TextEncoder, Registry};
use std::sync::OnceLock;
use opentelemetry::metrics::CallbackRegistration;

static UP_CB: OnceLock<Box<dyn CallbackRegistration>> = OnceLock::new();

// Initialize OTEL metrics and expose Prometheus /metrics endpoint
pub fn init_metrics(node_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Metrics: Prometheus exporter (served by this process on /metrics)
    let registry = Registry::new();
    let exporter = otel_prom_exporter().with_registry(registry.clone()).build()?;

    let meter_provider = SdkMeterProvider::builder()
        .with_reader(exporter)
        .build();
    global::set_meter_provider(meter_provider);

    // 'up' metric: observable gauge that always reports 1
    let meter = opentelemetry::global::meter("leslie");
    let up = meter
        .u64_observable_gauge("up")
        .with_description("1 if the service is up")
        .init();
    let node = node_id.to_string();
    let reg = meter.register_callback(&[up.as_any()], move |observer| {
        observer.observe_u64(&up, 1, &[KeyValue::new("node_id", node.clone())]);
    })?;
    let _ = UP_CB.set(reg);

    // spawn metrics HTTP server
    let addr: SocketAddr = std::env::var("PROMETHEUS_BIND")
        .unwrap_or_else(|_| "0.0.0.0:9464".to_string())
        .parse()?;
    let app = {
        let registry = registry.clone();
        Router::new().route(
            "/metrics",
            get(move || {
                let registry = registry.clone();
                async move {
                    let metric_families = registry.gather();
                    let mut buffer = Vec::new();
                    let encoder = TextEncoder::new();
                    if encoder.encode(&metric_families, &mut buffer).is_ok() {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, encoder.format_type())
                            .body(Body::from(buffer))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .header(header::CONTENT_TYPE, "text/plain")
                            .body(Body::from("encode error"))
                            .unwrap()
                    }
                }
            }),
        )
    };
    tokio::spawn(async move {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                tracing::info!("metrics listening on {}", listener.local_addr().ok());
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("metrics server exited: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("metrics bind failed: {}", e);
            }
        }
    });

    Ok(())
}
