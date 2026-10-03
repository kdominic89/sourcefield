//! Validated execution intent shared by collection and import resolution.

use anyhow::{Result, bail};

/// Whether a refresh may expose explicitly dated fallback observations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FailurePolicy {
    /// Required sources and diagnostics must all satisfy publication policy.
    Strict,
    /// Local previews may retain dated data when collection fails.
    AllowFallback,
}

/// Mutually exclusive execution intents, constructed once at the CLI boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionMode {
    /// Refresh observations with an explicit failure contract.
    Refresh(FailurePolicy),
    /// Build a clearly marked preview without network access.
    OfflinePreview,
    /// Reproduce an approved capture exactly without new authorization or network access.
    LockedReplay,
}

impl ExecutionMode {
    /// Reject contradictory public flags before opening files or constructing transports.
    pub(crate) fn from_flags(offline: bool, locked: bool, strict_live: bool) -> Result<Self> {
        if offline && strict_live {
            bail!("--offline and --strict-live are mutually exclusive");
        }

        if locked && !offline {
            bail!("--locked requires --offline; refresh and replay are distinct operations");
        }

        Ok(match (offline, locked, strict_live) {
            (true, true, false) => Self::LockedReplay,
            (true, false, false) => Self::OfflinePreview,
            (false, false, true) => Self::Refresh(FailurePolicy::Strict),
            (false, false, false) => Self::Refresh(FailurePolicy::AllowFallback),
            _ => unreachable!("invalid flag combinations were rejected above"),
        })
    }

    /// Offline intent excludes HTTP client construction as well as requests.
    pub(crate) fn is_offline(self) -> bool {
        matches!(self, Self::OfflinePreview | Self::LockedReplay)
    }

    /// Replay consumes immutable captured inputs rather than current local configuration facts.
    pub(crate) fn is_replay(self) -> bool {
        self == Self::LockedReplay
    }

    /// Strict collection must reject incomplete sources and warnings before publication.
    pub(crate) fn is_strict(self) -> bool {
        self == Self::Refresh(FailurePolicy::Strict)
    }

    /// Only an explicitly permissive refresh can use dated observations after failure.
    pub(crate) fn permits_fallback(self) -> bool {
        self == Self::Refresh(FailurePolicy::AllowFallback)
    }

    /// Online imports require immutable provenance even for permissive local refreshes.
    pub(crate) fn requires_published_imports(self) -> bool {
        matches!(self, Self::Refresh(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_flags_select_one_execution_contract() {
        let cases = [
            (
                (false, false, false),
                ExecutionMode::Refresh(FailurePolicy::AllowFallback),
            ),
            (
                (false, false, true),
                ExecutionMode::Refresh(FailurePolicy::Strict),
            ),
            ((true, false, false), ExecutionMode::OfflinePreview),
            ((true, true, false), ExecutionMode::LockedReplay),
        ];

        let actual = cases.map(|((offline, locked, strict), _)| {
            ExecutionMode::from_flags(offline, locked, strict).unwrap()
        });

        assert_eq!(actual, cases.map(|(_, expected)| expected));
    }

    #[test]
    fn invalid_flags_are_rejected_at_construction() {
        let cases = [
            (false, true, false),
            (false, true, true),
            (true, false, true),
            (true, true, true),
        ];

        let results = cases
            .map(|(offline, locked, strict)| ExecutionMode::from_flags(offline, locked, strict));

        assert!(results.iter().all(Result::is_err));
    }
}
