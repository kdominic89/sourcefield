//! Select observations from validated execution intent without constructing unused resources.

use std::future::Future;

use anyhow::{Result, ensure};
use sourcefield_core::{DataStatus, Snapshot, SnapshotMode, usable_observation};

use crate::modes::ExecutionMode;

/// Public diagnostic for a retained observation after total collection failure.
pub(crate) const FALLBACK_WARNING: &str =
    "Live collection failed; using the dated fallback snapshot.";
/// Public diagnostic for deliberate preview generation.
pub(crate) const PREVIEW_WARNING: &str =
    "Offline preview: public signals are replaced on the first successful live workflow.";
/// Public diagnostic for requested but unauthorized aggregate collection.
pub(crate) const PRIVATE_COUNT_WARNING: &str = concat!(
    "Aggregate private repository counts were requested but PROFILE_TOKEN is not ",
    "configured; the private signal remains sealed."
);

/// Retain one copy of an owned diagnostic, preserving other warnings and their order.
pub(crate) fn add_warning(snapshot: &mut Snapshot, warning: &str) {
    let mut present = false;
    snapshot.warnings.retain(|existing| {
        if existing != warning {
            return true;
        }

        let first = !present;
        present = true;

        first
    });

    // Historical captures may already contain duplicates; normalize only this owned message.
    if !present {
        snapshot.warnings.push(warning.to_string());
    }
}

/// Invoke live collection only for refresh; preserve dated fallback provenance on failure.
///
/// The closure is the resource factory: offline/replay never construct a collector or HTTP client.
/// Static dispatch avoids allocating a boxed future or transport for modes that do not use one.
pub(crate) async fn acquire<F, Fut>(
    mode: ExecutionMode,
    captured: &Snapshot,
    collect: F,
) -> Result<Snapshot>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Snapshot>>,
{
    match mode {
        ExecutionMode::LockedReplay => Ok(captured.clone()),
        ExecutionMode::OfflinePreview => {
            let mut captured = captured.clone();
            captured.mode = SnapshotMode::Preview;
            captured.fetched_at.clear();
            for source in &mut captured.sources {
                source.status = DataStatus::Preview;
            }

            add_warning(&mut captured, PREVIEW_WARNING);

            Ok(captured)
        }
        ExecutionMode::Refresh(_) => match collect().await {
            Ok(snapshot) => Ok(snapshot),
            Err(_) if mode.permits_fallback() => {
                ensure!(
                    usable_observation(captured),
                    "live collection failed and no dated observation is available for fallback"
                );

                let mut captured = captured.clone();
                captured.mode = SnapshotMode::Fallback;
                for source in &mut captured.sources {
                    source.status = DataStatus::Fallback;
                }

                // Keep the original observation date; clearing it would make dated fallback opaque.
                add_warning(&mut captured, FALLBACK_WARNING);

                Ok(captured)
            }
            Err(error) => Err(error.context("collect live public signals")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modes::FailurePolicy;

    #[tokio::test]
    async fn replay_never_constructs_the_collector() {
        let captured = Snapshot {
            private_repository_count: Some(7),
            ..Snapshot::default()
        };
        let called = std::cell::Cell::new(false);

        let result = acquire(ExecutionMode::LockedReplay, &captured, || {
            called.set(true);
            std::future::ready(Ok(Snapshot::default()))
        })
        .await
        .unwrap();

        assert!(!called.get());
        assert_eq!(result.private_repository_count, Some(7));
    }

    #[tokio::test]
    async fn offline_never_invokes_live_collection() {
        let called = std::cell::Cell::new(false);

        let result = acquire(ExecutionMode::OfflinePreview, &Snapshot::default(), || {
            called.set(true);
            std::future::ready(Ok(Snapshot::default()))
        })
        .await
        .unwrap();

        assert!(!called.get());
        assert_eq!(result.mode, SnapshotMode::Preview);
    }

    #[tokio::test]
    async fn permissive_failure_retains_dated_capture() {
        let captured = Snapshot {
            mode: SnapshotMode::Live,
            fetched_at: "2026-10-01T00:00:00Z".into(),
            ..Snapshot::default()
        };

        let result = acquire(
            ExecutionMode::Refresh(FailurePolicy::AllowFallback),
            &captured,
            || async { anyhow::bail!("simulated unavailable upstream") },
        )
        .await
        .unwrap();

        assert_eq!(result.mode, SnapshotMode::Fallback);
        assert_eq!(result.fetched_at, "2026-10-01T00:00:00Z");
        assert_eq!(result.warnings.len(), 1);
    }

    #[tokio::test]
    async fn strict_failure_never_returns_capture() {
        let mode = ExecutionMode::Refresh(FailurePolicy::Strict);

        let result = acquire(mode, &Snapshot::default(), || async {
            anyhow::bail!("simulated unavailable upstream")
        })
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn successful_refresh_returns_current_observation() {
        let mode = ExecutionMode::Refresh(FailurePolicy::Strict);
        let live = Snapshot {
            mode: SnapshotMode::Live,
            fetched_at: "2026-10-03T00:00:00Z".into(),
            ..Snapshot::default()
        };

        let result = acquire(mode, &Snapshot::default(), || std::future::ready(Ok(live)))
            .await
            .unwrap();

        assert_eq!(result.mode, SnapshotMode::Live);
        assert_eq!(result.fetched_at, "2026-10-03T00:00:00Z");
    }
    #[tokio::test]
    async fn permissive_failure_rejects_undated_seed() {
        let mode = ExecutionMode::Refresh(FailurePolicy::AllowFallback);

        let result = acquire(mode, &Snapshot::default(), || async {
            anyhow::bail!("simulated unavailable upstream")
        })
        .await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no dated observation")
        );
    }

    #[tokio::test]
    async fn permissive_refresh_preserves_explicit_partial_status() {
        let mode = ExecutionMode::Refresh(FailurePolicy::AllowFallback);
        let partial = Snapshot {
            mode: SnapshotMode::Partial,
            ..Snapshot::default()
        };

        let result = acquire(mode, &Snapshot::default(), || {
            std::future::ready(Ok(partial))
        })
        .await
        .unwrap();

        assert_eq!(result.mode, SnapshotMode::Partial);
    }
}
