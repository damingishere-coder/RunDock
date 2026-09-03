// @group BusinessLogic > HealthCheck : HTTP/TCP health check probe loop

use crate::config::notification_store::NotificationsStore;
use crate::models::process_info::HealthCheckStatus;
use crate::models::process_status::ProcessStatus;
use crate::notifications::sender::{fire_event, ProcessEvent};
use crate::process::instance::ManagedProcess;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// @group BusinessLogic > HealthCheck : Probe a URL via HTTP GET or TCP connect
async fn probe(url: &str, timeout_secs: u64) -> bool {
    let timeout_dur = Duration::from_secs(timeout_secs);

    if url.starts_with("http://") || url.starts_with("https://") {
        let Ok(url) = crate::utils::outbound::validate_url(
            url,
            crate::utils::outbound::OutboundPolicy::LoopbackHttp,
        ) else {
            return false;
        };
        let Ok(client) = crate::utils::outbound::client_for_url(
            &url,
            crate::utils::outbound::OutboundPolicy::LoopbackHttp,
        )
        .await
        else {
            return false;
        };
        // HTTP probe — pinned to loopback DNS and redirects disabled.
        match tokio::time::timeout(timeout_dur, async {
            client.get(url).timeout(timeout_dur).send().await
        })
        .await
        {
            Ok(Ok(resp)) => resp.status().is_success(),
            _ => false,
        }
    } else {
        // TCP probes are also local-process checks. Resolve first and reject
        // any target set containing a non-loopback address.
        let Ok(Ok(addresses)) =
            tokio::time::timeout(timeout_dur, tokio::net::lookup_host(url)).await
        else {
            return false;
        };
        let addresses: Vec<_> = addresses.collect();
        if addresses.is_empty() || addresses.iter().any(|address| !address.ip().is_loopback()) {
            return false;
        }
        matches!(
            tokio::time::timeout(timeout_dur, tokio::net::TcpStream::connect(addresses[0])).await,
            Ok(Ok(_))
        )
    }
}

// @group BusinessLogic > HealthCheck : Spawn a health check loop for a process
pub fn start_health_check(
    arc: Arc<RwLock<ManagedProcess>>,
    expected_generation: u64,
    url: String,
    interval_secs: u64,
    timeout_secs: u64,
    retries: u32,
    notifications: Arc<RwLock<NotificationsStore>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut consecutive_failures: u32 = 0;
        let mut unhealthy_notified = false;
        let required_failures = retries.max(1);
        let mut next_delay = Duration::from_secs(interval_secs.max(1));

        // Wait briefly before the first probe so the process has time to bind its port
        tokio::time::sleep(Duration::from_secs(interval_secs.min(5))).await;

        loop {
            tokio::time::sleep(next_delay).await;

            // Only probe when the process is actively running
            let should_check = {
                let proc = arc.read().await;
                proc.generation == expected_generation
                    && proc.config.enabled
                    && proc.desired_running
                    && matches!(
                        proc.status,
                        ProcessStatus::Running | ProcessStatus::Watching
                    )
            };
            if !should_check {
                return;
            }

            let healthy = probe(&url, timeout_secs).await;

            if healthy {
                if unhealthy_notified {
                    let name = {
                        let process = arc.read().await;
                        if process.generation != expected_generation
                            || !process.config.enabled
                            || !process.desired_running
                        {
                            return;
                        }
                        process.config.name.clone()
                    };
                    tracing::info!(
                        "process '{}' health check recovered after {} failures",
                        name,
                        consecutive_failures
                    );
                    let info = {
                        let mut process = arc.write().await;
                        if process.generation != expected_generation
                            || !process.config.enabled
                            || !process.desired_running
                        {
                            return;
                        }
                        process.health_status = Some(HealthCheckStatus::Healthy);
                        process.to_info()
                    };
                    if arc.read().await.generation != expected_generation {
                        return;
                    }
                    let store = notifications.read().await;
                    fire_event(&store, &info, ProcessEvent::HealthRecovered).await;
                    unhealthy_notified = false;
                } else {
                    let mut process = arc.write().await;
                    if process.generation != expected_generation
                        || !process.config.enabled
                        || !process.desired_running
                    {
                        return;
                    }
                    process.health_status = Some(HealthCheckStatus::Healthy);
                }
                consecutive_failures = 0;
                next_delay = Duration::from_secs(interval_secs.max(1));
            } else {
                consecutive_failures = consecutive_failures.saturating_add(1);
                let name = {
                    let process = arc.read().await;
                    if process.generation != expected_generation
                        || !process.config.enabled
                        || !process.desired_running
                    {
                        return;
                    }
                    process.config.name.clone()
                };
                tracing::warn!(
                    "process '{}' health check failed ({}/{})",
                    name,
                    consecutive_failures,
                    required_failures
                );

                if consecutive_failures >= required_failures {
                    let info = {
                        let mut process = arc.write().await;
                        if process.generation != expected_generation
                            || !process.config.enabled
                            || !process.desired_running
                        {
                            return;
                        }
                        process.health_status = Some(HealthCheckStatus::Unhealthy);
                        (!unhealthy_notified).then(|| process.to_info())
                    };
                    next_delay =
                        health_backoff(interval_secs, consecutive_failures, required_failures);

                    if let Some(info) = info {
                        if arc.read().await.generation != expected_generation {
                            return;
                        }
                        let store = notifications.read().await;
                        fire_event(&store, &info, ProcessEvent::Unhealthy).await;
                        unhealthy_notified = true;
                    }
                }
            }
        }
    })
}

fn health_backoff(interval_secs: u64, failures: u32, threshold: u32) -> Duration {
    let exponent = failures.saturating_sub(threshold).min(4);
    let multiplier = 1_u64 << exponent;
    Duration::from_secs(interval_secs.max(1).saturating_mul(multiplier).min(300))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unhealthy_probe_backoff_is_bounded_and_resets_from_threshold() {
        assert_eq!(health_backoff(10, 3, 3), Duration::from_secs(10));
        assert_eq!(health_backoff(10, 4, 3), Duration::from_secs(20));
        assert_eq!(health_backoff(60, 20, 3), Duration::from_secs(300));
    }
}
