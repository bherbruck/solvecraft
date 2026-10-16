// SPDX-License-Identifier: Apache-2.0
//! Part 21 grammar selected by a `FILE_DESCRIPTION` declaration.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImplementationLevel {
    LegacyEdition1,
    LegacyEdition2,
    Edition3Class1,
    Edition3Class2,
    Edition3Class3,
}

impl ImplementationLevel {
    fn known(declaration: &str) -> Option<Self> {
        match declaration {
            "1" | "2" | "2;1" | "2;2" => Some(Self::LegacyEdition1),
            "3;1" | "3;2" => Some(Self::LegacyEdition2),
            "4;1" => Some(Self::Edition3Class1),
            "4;2" => Some(Self::Edition3Class2),
            "4;3" => Some(Self::Edition3Class3),
            _ => None,
        }
    }

    pub(crate) fn is_edition3(self) -> bool {
        matches!(
            self,
            Self::Edition3Class1 | Self::Edition3Class2 | Self::Edition3Class3
        )
    }

    pub(super) fn edition3_sections_forbidden_by(self) -> Option<&'static str> {
        match self {
            Self::LegacyEdition1 => Some("2;1"),
            Self::LegacyEdition2 => Some("3;1"),
            Self::Edition3Class1 => Some("4;1"),
            Self::Edition3Class2 | Self::Edition3Class3 => None,
        }
    }

    pub(super) fn class3_occurrence_restriction(self) -> Option<&'static str> {
        match self {
            Self::LegacyEdition1 | Self::LegacyEdition2 => {
                Some("historical implementation levels forbid edition-3 occurrence names")
            }
            Self::Edition3Class1 | Self::Edition3Class2 => {
                Some("this implementation level forbids value instances and EXPRESS constants")
            }
            Self::Edition3Class3 => None,
        }
    }
}

/// An implementation-level declaration and its admitted grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeclaredImplementationLevel {
    text: String,
    known: Option<ImplementationLevel>,
}

impl DeclaredImplementationLevel {
    /// A declaration with its grammar resolved from the text.
    pub(super) fn new(text: String) -> Self {
        Self {
            known: ImplementationLevel::known(&text),
            text,
        }
    }

    /// The verbatim declaration text.
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    /// The grammar applied to the declaration.
    pub(super) fn level(&self) -> ImplementationLevel {
        self.known.unwrap_or(ImplementationLevel::Edition3Class3)
    }

    /// Whether the declaration has no known grammar.
    pub(super) fn is_unverified(&self) -> bool {
        self.known.is_none()
    }
}
