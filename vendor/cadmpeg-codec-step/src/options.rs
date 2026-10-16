// SPDX-License-Identifier: Apache-2.0
//! Writer targets and header metadata.
//!
//! These configure *how* a target is written. Which target is written is the
//! export request's answer, resolved against the source: `StepCodec::plan`
//! takes it from `TargetRequest`.

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::TargetDescriptor;

use crate::dialect::{
    STEP_AP203_E1, STEP_AP203_E2, STEP_AP214, STEP_AP242_E1, STEP_AP242_E2, STEP_AP242_E3,
};

/// Metadata written to the STEP `FILE_NAME` header record.
///
/// Default values produce deterministic output. They identify the file as
/// `cadmpeg_model`, leave the author and organization empty, use `cadmpeg` as
/// the originating system, and substitute `1970-01-01T00:00:00` when the
/// timestamp is absent.
#[derive(Debug, Clone)]
pub struct StepWriteOptions {
    /// The `FILE_NAME` name field.
    ///
    /// The STEP `PRODUCT` id and name come from the first IR body name, or
    /// `cadmpeg_model` when that body has no name.
    pub product_name: String,
    /// The sole entry in the `FILE_NAME` author list.
    pub author: String,
    /// The sole entry in the `FILE_NAME` organization list.
    pub organization: String,
    /// The `FILE_NAME` timestamp.
    ///
    /// Supply an ISO 8601 value. Absence writes `1970-01-01T00:00:00`.
    pub timestamp: Option<String>,
    /// The `FILE_NAME` originating-system field.
    pub originating_system: String,
}

impl Default for StepWriteOptions {
    fn default() -> Self {
        StepWriteOptions {
            product_name: "cadmpeg_model".to_string(),
            author: String::new(),
            organization: String::new(),
            timestamp: None,
            originating_system: "cadmpeg".to_string(),
        }
    }
}

/// STEP application-protocol targets supported by the Part 21 writer.
///
/// The AP242 edition number and the long-form schema revision are distinct:
/// editions 1, 2, and 3 use long-form revisions 1, 3, and 4 respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StepSchema {
    /// AP203 edition 1 `CONFIG_CONTROL_DESIGN`.
    Ap203Edition1,
    /// AP203 edition 2 modular long form.
    Ap203Edition2,
    /// AP214 `AUTOMOTIVE_DESIGN`.
    Ap214,
    /// AP242 edition 1 modular long form.
    Ap242Edition1,
    /// AP242 edition 2 modular long form.
    Ap242Edition2,
    /// AP242 edition 3 modular long form.
    Ap242Edition3,
}

macro_rules! writer_vocabulary {
    ($(#[$all_meta:meta])* $count:literal; $($variant:ident),+ $(,)?) => {
        $(#[$all_meta])*
        pub(crate) const ALL: [Self; $count] = [$(Self::$variant),+];
        /// The generic encoder view projected from [`Self::ALL`].
        pub(crate) const TARGETS: &'static [TargetDescriptor] = &[
            $(Self::$variant.descriptor()),+
        ];
        /// The schema at a generic encoder target index.
        pub(crate) const fn from_target_index(index: usize) -> Option<Self> {
            if index < Self::ALL.len() {
                Some(Self::ALL[index])
            } else {
                None
            }
        }
    };
}

impl StepSchema {
    writer_vocabulary!(
        /// Every schema the Part 21 writer can emit, and so every row of the
        /// synthesis catalog. The same invocation projects the generic encoder
        /// catalog, so adding a typed schema cannot omit its target descriptor.
        /// Every row names the exact `FILE_SCHEMA` declaration this enum writes;
        /// AP214 is the cross-format default.
        6;
        Ap203Edition1,
        Ap203Edition2,
        Ap214,
        Ap242Edition1,
        Ap242Edition2,
        Ap242Edition3,
    );

    /// Exact schema identifier written in `FILE_SCHEMA`.
    pub const fn file_schema(self) -> &'static str {
        match self {
            Self::Ap203Edition1 => "CONFIG_CONTROL_DESIGN",
            Self::Ap203Edition2 => "AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF { 1 0 10303 403 2 1 2 }",
            Self::Ap214 => "AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }",
            Self::Ap242Edition1 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }",
            Self::Ap242Edition2 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 3 1 4 }",
            Self::Ap242Edition3 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }",
        }
    }

    /// The typed dialect identity written for this schema.
    pub(crate) const fn id(self) -> DialectId {
        match self {
            Self::Ap203Edition1 => STEP_AP203_E1,
            Self::Ap203Edition2 => STEP_AP203_E2,
            Self::Ap214 => STEP_AP214,
            Self::Ap242Edition1 => STEP_AP242_E1,
            Self::Ap242Edition2 => STEP_AP242_E2,
            Self::Ap242Edition3 => STEP_AP242_E3,
        }
    }

    /// The typed write-target catalog row for this schema.
    #[must_use]
    pub const fn descriptor(self) -> TargetDescriptor {
        let aliases = match self {
            Self::Ap203Edition1 => &["ap203e1"].as_slice(),
            Self::Ap203Edition2 => &["ap203e2"].as_slice(),
            Self::Ap214 => &["ap214"].as_slice(),
            Self::Ap242Edition1 => &["ap242e1"].as_slice(),
            Self::Ap242Edition2 => &["ap242e2"].as_slice(),
            Self::Ap242Edition3 => &["ap242e3"].as_slice(),
        };
        TargetDescriptor {
            id: self.id(),
            aliases,
        }
    }

    pub(crate) const fn supports_tessellation(self) -> bool {
        matches!(
            self,
            Self::Ap242Edition1 | Self::Ap242Edition2 | Self::Ap242Edition3
        )
    }

    pub(crate) const fn supports_semantic_pmi(self) -> bool {
        self.supports_tessellation()
    }

    pub(crate) const fn supports_visibility(self) -> bool {
        !matches!(self, Self::Ap203Edition1)
    }

    pub(crate) const fn application_protocol(self) -> (&'static str, &'static str, i32) {
        match self {
            Self::Ap203Edition1 => (
                "configuration controlled 3d designs of mechanical parts and assemblies",
                "config_control_design",
                1994,
            ),
            Self::Ap203Edition2 => (
                "configuration controlled 3d designs of mechanical parts and assemblies",
                "ap203_configuration_controlled_3d_design_of_mechanical_parts_and_assemblies",
                2011,
            ),
            Self::Ap214 => ("automotive design", "automotive_design", 2000),
            Self::Ap242Edition1 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2014,
            ),
            Self::Ap242Edition2 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2020,
            ),
            Self::Ap242Edition3 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2022,
            ),
        }
    }
}
