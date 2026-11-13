use opentelemetry::{global, trace::TracerProvider as _};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, SpanExporter, WithExportConfig};
use opentelemetry_otlp::MetricExporter;
use opentelemetry_sdk::{
    logs::SdkLoggerProvider, propagation::TraceContextPropagator, trace::SdkTracerProvider,
    Resource,
};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use tracing_subscriber::{fmt, prelude::*, EnvFilter, Layer};


pub struct Telemetry {
    tracer: SdkTracerProvider,
    logger: SdkLoggerProvider,
    meter: SdkMeterProvider,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Call BEFORE creating the Tokio runtime (the blocking HTTP client must not be built inside one).
pub fn init(service_name: &str) -> anyhow::Result<Telemetry> {
    // Full URLs, including the path. In-cluster defaults; override from the host.
    let logs_url = env_or(
        "OTLP_LOGS_ENDPOINT",
        "http://loki.monitoring.svc.cluster.local:3100/otlp/v1/logs",
    );
    let traces_url = env_or(
        "OTLP_TRACES_ENDPOINT",
        "http://otel-collector.monitoring.svc.cluster.local:4318/v1/traces",
    );
    let metrics_url = env_or(
        "OTLP_METRICS_ENDPOINT",
        "http://otel-collector.monitoring.svc.cluster.local:4318/v1/metrics",
    );

    let resource = Resource::builder().with_service_name(service_name.to_owned()).build();

    let tracer = SdkTracerProvider::builder()
        .with_batch_exporter(
            SpanExporter::builder().with_http().with_endpoint(traces_url).build()?,
        )
        .with_resource(resource.clone())
        .build();

    let logger = SdkLoggerProvider::builder()
        .with_batch_exporter(
            LogExporter::builder().with_http().with_endpoint(logs_url).build()?,
        )
        .with_resource(resource.clone())
        .build();

    let meter = SdkMeterProvider::builder()
        .with_periodic_exporter(
            MetricExporter::builder().with_http().with_endpoint(metrics_url).build()?,
        )
        .with_resource(resource)   // clone earlier, then move into the last builder
        .build();

    global::set_tracer_provider(tracer.clone());
    global::set_text_map_propagator(TraceContextPropagator::new());
    global::set_meter_provider(meter.clone());

    // Keep the exporters' own activity out of both OTel layers (feedback loop)
    let otel_filter = || {
        EnvFilter::new(
            "info,ur_scanner=debug,hyper=off,h2=off,reqwest=off,\
             opentelemetry=off,opentelemetry_sdk=off,opentelemetry_otlp=off",
        )
    };
    let stdout_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(fmt::layer().compact().with_filter(stdout_filter))
        .with(
            tracing_opentelemetry::layer()
                .with_tracer(tracer.tracer(service_name.to_owned()))
                .with_filter(otel_filter()),
        )
        .with(OpenTelemetryTracingBridge::new(&logger).with_filter(otel_filter()))
        .init();

    Ok(Telemetry { tracer, logger, meter })

}

impl Telemetry {
    /// Call AFTER block_on returns, outside the runtime. Flushes pending batches.
    pub fn shutdown(self) {
        if let Err(e) = self.tracer.shutdown() { eprintln!("tracer shutdown: {e}"); }
        if let Err(e) = self.logger.shutdown() { eprintln!("logger shutdown: {e}"); }
        if let Err(e) = self.meter.shutdown() { eprintln!("meter shutdown: {e}"); }


    }
}
