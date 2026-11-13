use opentelemetry::{
    global,
    metrics::{Counter, Gauge, Histogram},
    trace::Status,
    KeyValue,
};
use std::sync::OnceLock;
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub struct Metrics {
    pub scan_cycles: Counter<u64>,
    pub scan_duration: Histogram<f64>,
    pub errors: Counter<u64>,
    pub http_requests: Counter<u64>,
    pub last_success: Gauge<u64>,
}

pub fn metrics() -> &'static Metrics {
    static M: OnceLock<Metrics> = OnceLock::new();
    M.get_or_init(|| {
        let m = global::meter("ur-scraper");
        Metrics {
            scan_cycles: m.u64_counter("ur.scan.cycles").build(),
            scan_duration: m.f64_histogram("ur.scan.duration").with_unit("s").build(),
            errors: m.u64_counter("ur.errors").build(),
            http_requests: m.u64_counter("ur.http.client.requests").build(),
            last_success: m.u64_gauge("ur.scan.last_success").with_unit("s").build(),
        }
    })
}

/// Call once, at the boundary where an error is handled.
/// Counts it, marks the current span failed, and logs it (the log gets the trace_id).
pub fn record_error(kind: &'static str, err: &dyn std::fmt::Display) {
    metrics().errors.add(1, &[KeyValue::new("kind", kind)]);
    tracing::Span::current().set_status(Status::error(err.to_string()));
    tracing::error!(kind, error = %err, "operation failed");
}
