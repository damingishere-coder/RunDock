// @group BusinessLogic > Dependencies : Process dependency resolution

use crate::models::process_info::HealthCheckStatus;
use crate::models::process_status::ProcessStatus;
use crate::process::manager::ProcessRegistry;
use anyhow::{anyhow, Result};
use std::time::Duration;
use uuid::Uuid;

const DEPENDENCY_TIMEOUT_SECS: u64 = 60;
const POLL_INTERVAL_MS: u64 = 500;

// @group BusinessLogic > Dependencies : Wait for all dependencies to be Running/Healthy
pub async fn wait_for_dependencies(
    depends_on: &[String],
    registry: &ProcessRegistry,
    timeout: Duration,
) -> Result<()> {
    for dep_name in depends_on {
        let dep_id = find_by_name(registry, dep_name)
            .await?
            .ok_or_else(|| anyhow!("dependency '{}' not found in registry", dep_name))?;

        let deadline = tokio::time::Instant::now() + timeout;

        tracing::info!("waiting for dependency '{dep_name}' to be ready...");

        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(anyhow!(
                    "timed out waiting for dependency '{}' after {}s",
                    dep_name,
                    timeout.as_secs()
                ));
            }

            if let Some(arc) = registry.get(&dep_id) {
                let proc = arc.read().await;
                let is_running = matches!(
                    proc.status,
                    ProcessStatus::Running | ProcessStatus::Watching
                );

                if is_running {
                    // If the dependency has a health check, also wait for Healthy
                    if proc.config.health_check_url.is_some() {
                        if proc.health_status == Some(HealthCheckStatus::Healthy) {
                            tracing::info!("dependency '{dep_name}' is healthy");
                            break;
                        }
                    } else {
                        tracing::info!("dependency '{dep_name}' is running");
                        break;
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
        }
    }

    Ok(())
}

// @group BusinessLogic > Dependencies : Default timeout for dependency resolution
pub fn default_timeout() -> Duration {
    Duration::from_secs(DEPENDENCY_TIMEOUT_SECS)
}

async fn find_by_name(registry: &ProcessRegistry, name: &str) -> Result<Option<Uuid>> {
    let mut matched = None;
    for entry in registry.iter() {
        let proc = entry.value().read().await;
        if proc.config.name == name {
            if matched.is_some() {
                return Err(anyhow!(
                    "multiple processes are named '{name}'; dependency targets must be unique"
                ));
            }
            matched = Some(*entry.key());
        }
    }
    Ok(matched)
}
