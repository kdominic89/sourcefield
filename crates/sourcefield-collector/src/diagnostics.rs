//! Stable collector warning text shared with privacy-aware publication diagnostics.

use std::fmt;

/// A recognized collector warning; borrowed identifiers remain untrusted until authorized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectorWarning<'a> {
    /// Repository exhaustion was not proven within configured bounds.
    RepositoryLimit,
    /// A requested source was unavailable.
    SourceUnavailable(&'a str),
    /// NuGet did not return the exact requested package and owner.
    NugetNoExactMatch(&'a str),
    /// NuGet metadata could not be retrieved or decoded.
    NugetMetadataUnavailable(&'a str),
}

impl<'a> CollectorWarning<'a> {
    /// Recognize historical warning text without allocating or trusting its identifier.
    /// Consumers must authorize identifiers before including them in public diagnostics.
    pub fn parse(text: &'a str) -> Option<Self> {
        if text == Self::RepositoryLimit.prefix() {
            return Some(Self::RepositoryLimit);
        }

        // One grammar owns both serialization and recognition, preserving old capture bytes.
        for kind in [
            Self::SourceUnavailable(""),
            Self::NugetNoExactMatch(""),
            Self::NugetMetadataUnavailable(""),
        ] {
            if let Some(identifier) = text.strip_prefix(kind.prefix()) {
                return match kind {
                    Self::SourceUnavailable(_) => Some(Self::SourceUnavailable(identifier)),
                    Self::NugetNoExactMatch(_) => Some(Self::NugetNoExactMatch(identifier)),
                    Self::NugetMetadataUnavailable(_) => {
                        Some(Self::NugetMetadataUnavailable(identifier))
                    }
                    Self::RepositoryLimit => None,
                };
            }
        }

        None
    }

    /// Return the single source of truth for each warning's historical wire prefix.
    fn prefix(self) -> &'static str {
        match self {
            Self::RepositoryLimit => {
                "Repository inventory reached its collection or pagination limit"
            }
            Self::SourceUnavailable(_) => "Source unavailable: ",
            Self::NugetNoExactMatch(_) => "NuGet returned no exact match for ",
            Self::NugetMetadataUnavailable(_) => "NuGet metadata unavailable: ",
        }
    }
}

impl fmt::Display for CollectorWarning<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.prefix())?;
        match self {
            Self::RepositoryLimit => Ok(()),
            Self::SourceUnavailable(identifier)
            | Self::NugetNoExactMatch(identifier)
            | Self::NugetMetadataUnavailable(identifier) => formatter.write_str(identifier),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CollectorWarning;

    #[test]
    fn historical_warning_text_round_trips() {
        let cases = [
            (
                CollectorWarning::RepositoryLimit,
                "Repository inventory reached its collection or pagination limit",
            ),
            (
                CollectorWarning::SourceUnavailable("github:repositories"),
                "Source unavailable: github:repositories",
            ),
            (
                CollectorWarning::NugetNoExactMatch("Example.Package"),
                "NuGet returned no exact match for Example.Package",
            ),
            (
                CollectorWarning::NugetMetadataUnavailable("Example.Package"),
                "NuGet metadata unavailable: Example.Package",
            ),
        ];

        let actual =
            cases.map(|(warning, text)| (warning.to_string(), CollectorWarning::parse(text)));

        assert_eq!(
            actual,
            cases.map(|(warning, text)| (text.to_string(), Some(warning)))
        );
    }

    #[test]
    fn unknown_warning_is_not_recognized() {
        let text = "Repository inventory reached its collection or pagination limit: secret";

        let actual = CollectorWarning::parse(text);

        assert_eq!(actual, None);
    }
}
