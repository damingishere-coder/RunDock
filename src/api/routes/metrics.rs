// @group APIEndpoints > Metrics : Prometheus text format metrics endpoint

use crate::daemon::state::DaemonState;
use crate::models::process_status::ProcessStatus;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Router};
use std::sync::Arc;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/metrics", get(prometheus_metrics))
        .with_state(state)
}

// @group APIEndpoints > Metrics : GET /metrics — Prometheus text exposition format
async fn prometheus_metrics(State(state): State<Arc<DaemonState>>) -> impl IntoResponse {
    let processes = state.manager.list().await;
    const MAX_METRIC_PROCESSES: usize = 1000;
    let exported_processes = &processes[..processes.len().min(MAX_METRIC_PROCESSES)];
    let mut out = String::with_capacity(4096);

    // Process-level metrics
    out.push_str("# HELP alter_process_cpu_percent CPU usage percentage per process\n");
    out.push_str("# TYPE alter_process_cpu_percent gauge\n");
    for p in exported_processes {
        if let Some(cpu) = p.cpu_percent {
            out.push_str(&format!(
                "alter_process_cpu_percent{{name=\"{}\",namespace=\"{}\"}} {:.2}\n",
                escape_label(&p.name),
                escape_label(&p.namespace),
                cpu
            ));
        }
    }

    out.push_str("# HELP alter_process_memory_bytes Resident memory in bytes per process\n");
    out.push_str("# TYPE alter_process_memory_bytes gauge\n");
    for p in exported_processes {
        if let Some(mem) = p.memory_bytes {
            out.push_str(&format!(
                "alter_process_memory_bytes{{name=\"{}\",namespace=\"{}\"}} {}\n",
                escape_label(&p.name),
                escape_label(&p.namespace),
                mem
            ));
        }
    }

    out.push_str("# HELP alter_process_restart_count Total restarts per process\n");
    out.push_str("# TYPE alter_process_restart_count counter\n");
    for p in exported_processes {
        out.push_str(&format!(
            "alter_process_restart_count{{name=\"{}\",namespace=\"{}\"}} {}\n",
            escape_label(&p.name),
            escape_label(&p.namespace),
            p.restart_count
        ));
    }

    out.push_str("# HELP alter_process_uptime_seconds Process uptime in seconds\n");
    out.push_str("# TYPE alter_process_uptime_seconds gauge\n");
    for p in exported_processes {
        if let Some(up) = p.uptime_secs {
            out.push_str(&format!(
                "alter_process_uptime_seconds{{name=\"{}\",namespace=\"{}\"}} {}\n",
                escape_label(&p.name),
                escape_label(&p.namespace),
                up
            ));
        }
    }

    out.push_str(
        "# HELP alter_process_status Process status (1=running or scheduled, 0=inactive)\n",
    );
    out.push_str("# TYPE alter_process_status gauge\n");
    for p in exported_processes {
        let active = matches!(
            p.status,
            ProcessStatus::Running | ProcessStatus::Watching | ProcessStatus::Sleeping
        );
        out.push_str(&format!(
            "alter_process_status{{name=\"{}\",namespace=\"{}\"}} {}\n",
            escape_label(&p.name),
            escape_label(&p.namespace),
            if active { 1 } else { 0 }
        ));
    }

    // Daemon-level metrics
    let uptime = (chrono::Utc::now() - state.started_at).num_seconds().max(0);
    out.push_str("# HELP alter_daemon_uptime_seconds Daemon uptime in seconds\n");
    out.push_str("# TYPE alter_daemon_uptime_seconds gauge\n");
    out.push_str(&format!("alter_daemon_uptime_seconds {}\n", uptime));

    out.push_str("# HELP alter_daemon_process_count Total registered processes\n");
    out.push_str("# TYPE alter_daemon_process_count gauge\n");
    out.push_str(&format!("alter_daemon_process_count {}\n", processes.len()));
    out.push_str("# HELP alter_metrics_processes_truncated Whether process samples were capped\n");
    out.push_str("# TYPE alter_metrics_processes_truncated gauge\n");
    out.push_str(&format!(
        "alter_metrics_processes_truncated {}\n",
        if processes.len() > MAX_METRIC_PROCESSES {
            1
        } else {
            0
        }
    ));

    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        out,
    )
}

fn escape_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::escape_label;

    #[test]
    fn prometheus_label_values_are_escaped() {
        assert_eq!(escape_label("a\\b\n\"c"), "a\\\\b\\n\\\"c");
    }
}
