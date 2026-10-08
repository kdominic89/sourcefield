//! Shared footer measurement and allocation-free text wrapping in design coordinates.

use crate::{InterestConfig, LearningConfig, PresentationConfig};

/// Conservative character budget for interest and hardware labels in a footer column.
///
/// Width is bounded without browser font metrics; both measurement and emission use this rule.
pub const FOOTER_LABEL_COLUMNS: usize = 40;
/// Maximum characters per proportional footer description.
pub const FOOTER_DETAIL_COLUMNS: usize = 58;
/// Maximum characters per larger learning destination label.
pub const FOOTER_LEARNING_COLUMNS: usize = 35;
/// Maximum characters per monospaced hardware detail.
pub const FOOTER_HARDWARE_COLUMNS: usize = 52;

/// Measured footer coordinates shared by state construction and SVG emission.
#[derive(Debug, Clone, Copy)]
pub struct PersonalFooterLayout {
    /// Baseline of the first platform line.
    pub platforms_y: f32,
    /// Baseline of the first platform note line.
    pub platform_note_y: f32,
    /// Baseline of the first learning note line.
    pub learning_note_y: f32,
    /// Bottom separator ordinate.
    pub separator_y: f32,
    /// Additional canvas height beyond the approved compact footer.
    pub extra_height: f32,
}

/// Borrowed lines wrapped at character-safe word boundaries, without copying their text.
pub struct WrappedLines<'a> {
    lines: std::str::Lines<'a>,
    remaining: Option<&'a str>,
    columns: usize,
}

/// Iterate display lines using the same measurement in the core and renderer.
pub fn wrapped_lines(text: &str, columns: usize) -> WrappedLines<'_> {
    WrappedLines {
        lines: text.lines(),
        remaining: None,
        columns: columns.max(1),
    }
}

impl<'a> Iterator for WrappedLines<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let line = self.remaining.take().or_else(|| self.lines.next())?;
        let Some((boundary, _)) = line.char_indices().nth(self.columns) else {
            return Some(line);
        };

        // Word boundaries retain readability; long identifiers still progress safely.
        let boundary = line[..boundary]
            .rfind(char::is_whitespace)
            .filter(|index| *index > 0)
            .unwrap_or(boundary);

        let (head, tail) = line.split_at(boundary);
        let tail = tail.trim_start();
        // Whitespace discarded at a wrap boundary is not an authored empty physical line.
        self.remaining = (!tail.is_empty()).then_some(tail);

        Some(head)
    }
}

/// Advance a visible interest row using the compact, description-free lead-row contract.
pub fn next_visible_interest_y(y: f32, interest: &InterestConfig, row: usize) -> f32 {
    let labels = wrapped_lines(&interest.label, FOOTER_LABEL_COLUMNS)
        .count()
        .max(1);

    let summaries = if row == 0 {
        0
    } else {
        wrapped_lines(&interest.summary, FOOTER_DETAIL_COLUMNS).count()
    };

    let label_end = y + labels.saturating_sub(1) as f32 * 24.0;
    let summary_end = if summaries == 0 {
        label_end
    } else {
        label_end + 25.0 + summaries.saturating_sub(1) as f32 * 22.0
    };

    (y + 46.0).max(summary_end + 21.0)
}

/// Measure all visible footer columns while retaining approved compact baselines.
pub fn personal_footer_layout(
    presentation: &PresentationConfig,
    interests: &[InterestConfig],
    learning: &[LearningConfig],
    show_interests: bool,
) -> PersonalFooterLayout {
    let interests_end = interests
        .iter()
        .filter(|item| show_interests && item.show_in_readme)
        .enumerate()
        .fold(1422.0, |y, (row, interest)| {
            next_visible_interest_y(y, interest, row)
        });

    let platforms_y = 1542.0_f32.max(interests_end + 28.0);
    let platforms = presentation.platforms.join(" / ");
    let platform_lines = wrapped_lines(&platforms, FOOTER_LABEL_COLUMNS)
        .count()
        .max(1);

    let platform_note_y = platforms_y + platform_lines as f32 * 30.0;

    let platform_end = platform_note_y
        + wrapped_lines(&presentation.platform_note, FOOTER_DETAIL_COLUMNS)
            .count()
            .saturating_sub(1) as f32
            * 22.0;

    let learning_end = learning.iter().fold(1424.0, |y, item| {
        y + (wrapped_lines(&item.label, FOOTER_LEARNING_COLUMNS)
            .count()
            .max(1) as f32
            * 27.0
            + 13.0)
    });

    let learning_note_y = 1510.0_f32.max(learning_end + 6.0);
    let learning_bottom = learning_note_y
        + wrapped_lines(&presentation.learning_note, FOOTER_DETAIL_COLUMNS)
            .count()
            .saturating_sub(1) as f32
            * 30.0;

    let hardware_end = presentation.hardware.iter().fold(1422.0, |y, item| {
        let labels = wrapped_lines(&item.label, FOOTER_LABEL_COLUMNS)
            .count()
            .max(1);

        let details = wrapped_lines(&item.detail, FOOTER_HARDWARE_COLUMNS)
            .count()
            .max(1);

        y + 67.0 + labels.saturating_sub(1) as f32 * 24.0 + details.saturating_sub(1) as f32 * 22.0
    });

    let separator_y = 1620.0_f32
        .max(platform_end + 48.0)
        .max(learning_bottom + 50.0)
        .max(hardware_end - 3.0);

    PersonalFooterLayout {
        platforms_y,
        platform_note_y,
        learning_note_y,
        separator_y,
        extra_height: separator_y - 1620.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_identifiers_wrap_on_unicode_boundaries_without_copying() {
        let input = "a\u{00e9}bc\u{00e9}d";

        let lines = wrapped_lines(input, 3).collect::<Vec<_>>();

        assert_eq!(lines, ["a\u{00e9}b", "c\u{00e9}d"]);
        assert_eq!(lines.concat(), input);
    }

    #[test]
    fn zero_columns_still_make_progress() {
        let input = "abc";

        let lines = wrapped_lines(input, 0).collect::<Vec<_>>();

        assert_eq!(lines, ["a", "b", "c"]);
    }

    #[test]
    fn lead_interest_description_does_not_allocate_hidden_footer_space() {
        // Arrange
        let interest = InterestConfig {
            id: "lead".into(),
            label: "Overview".into(),
            summary: "hidden detail ".repeat(200),
            show_in_readme: true,
        };

        // Act
        let layout = personal_footer_layout(&PresentationConfig::default(), &[interest], &[], true);

        // Assert
        assert_eq!(layout.extra_height, 0.0);
        assert_eq!(layout.platforms_y, 1542.0);
    }

    #[test]
    fn later_interest_description_expands_footer_space() {
        // Arrange
        let interests = [
            InterestConfig {
                id: "lead".into(),
                label: "Overview".into(),
                summary: String::new(),
                show_in_readme: true,
            },
            InterestConfig {
                id: "detail".into(),
                label: "Languages".into(),
                summary: "visible detail ".repeat(200),
                show_in_readme: true,
            },
        ];

        // Act
        let layout = personal_footer_layout(&PresentationConfig::default(), &interests, &[], true);

        // Assert
        assert!(layout.extra_height > 0.0);
        assert!(layout.platforms_y > 1542.0);
    }

    #[test]
    fn hidden_interests_do_not_expand_footer() {
        let interest = InterestConfig {
            id: "hidden".into(),
            label: "hidden ".repeat(200),
            summary: "detail ".repeat(200),
            show_in_readme: false,
        };

        let layout = personal_footer_layout(&PresentationConfig::default(), &[interest], &[], true);

        assert_eq!(layout.extra_height, 0.0);
        assert_eq!(layout.platforms_y, 1542.0);
    }

    #[test]
    fn long_hardware_details_expand_separator_after_every_row() {
        let presentation = PresentationConfig {
            hardware: (0..5)
                .map(|index| crate::HardwareConfig {
                    label: format!("Hardware {index}"),
                    detail: "detail ".repeat(40),
                })
                .collect(),
            ..PresentationConfig::default()
        };

        let layout = personal_footer_layout(&presentation, &[], &[], true);

        assert!(layout.extra_height > 0.0);
        assert_eq!(layout.separator_y, 1620.0 + layout.extra_height);
    }

    #[test]
    fn learning_notes_follow_all_wrapped_destinations() {
        let learning = (0..5)
            .map(|index| LearningConfig {
                id: format!("course-{index}"),
                label: "A longer learning destination label ".repeat(3),
                url: "https://example.com/course".into(),
                interest: "learning".into(),
            })
            .collect::<Vec<_>>();

        let presentation = PresentationConfig {
            learning_note: "Final learning note".into(),
            ..PresentationConfig::default()
        };

        let layout = personal_footer_layout(&presentation, &[], &learning, true);

        assert!(layout.learning_note_y > 1510.0);
        assert!(layout.separator_y >= layout.learning_note_y + 50.0);
    }
}
