// SPDX-License-Identifier: Apache-2.0
//! Decode modes, resource ceilings, and input-proportional allowances.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// Whether a successful decode may report mandatory transfer losses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DecodeMode {
    /// Reject a completed decode that reports a mandatory transfer loss.
    Strict,
    /// Return the decoded result with its loss report.
    #[default]
    Salvage,
}

/// Absolute resource ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    /// Maximum physical input bytes read at the root.
    pub max_input_bytes: u64,
    /// Maximum cumulative decompressed bytes across all expansions.
    pub max_decompressed_bytes_total: u64,
    /// Maximum decompressed bytes produced by any single expansion.
    pub max_decompressed_bytes_per_expand: u64,
    /// Maximum bytes held by temporary materializations at one time.
    #[serde(default = "default_max_materialized_bytes")]
    pub max_materialized_bytes: u64,
    /// Maximum cumulative bytes retained outside expansion output.
    #[serde(default = "default_max_retained_bytes")]
    pub max_retained_bytes: u64,
    /// Maximum admitted semantic or native entities.
    #[serde(default = "default_max_entities")]
    pub max_entities: u64,
    /// Maximum admitted items across decoded collections.
    #[serde(default = "default_max_collection_items")]
    pub max_collection_items: u64,
    /// Maximum active recursive nesting depth.
    #[serde(default = "default_max_recursion_depth")]
    pub max_recursion_depth: u64,
    /// Maximum cumulative algorithm work units.
    #[serde(default = "default_max_work_units")]
    pub max_work_units: u64,
}

impl ResourceLimits {
    /// Generous ceilings for interactive desktop use; the default profile.
    pub const fn desktop() -> Self {
        Self {
            max_input_bytes: 4 * GIB,
            max_decompressed_bytes_total: 8 * GIB,
            max_decompressed_bytes_per_expand: 2 * GIB,
            max_materialized_bytes: 4 * GIB,
            max_retained_bytes: 4 * GIB,
            max_entities: 64_000_000,
            max_collection_items: 16_000_000,
            max_recursion_depth: 256,
            max_work_units: 4_000_000_000,
        }
    }

    /// Tight ceilings for unattended service use.
    pub const fn service() -> Self {
        Self {
            max_input_bytes: 256 * MIB,
            max_decompressed_bytes_total: GIB,
            max_decompressed_bytes_per_expand: 256 * MIB,
            max_materialized_bytes: 512 * MIB,
            max_retained_bytes: 512 * MIB,
            max_entities: 8_000_000,
            max_collection_items: 1_000_000,
            max_recursion_depth: 128,
            max_work_units: 256_000_000,
        }
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self::desktop()
    }
}

/// Decode mode and resource ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DecodePolicy {
    /// How mandatory transfer losses are handled.
    pub mode: DecodeMode,
    /// The absolute ceilings.
    pub limits: ResourceLimits,
}

impl DecodePolicy {
    /// The default desktop profile in salvage mode.
    pub const fn desktop() -> Self {
        Self {
            mode: DecodeMode::Salvage,
            limits: ResourceLimits::desktop(),
        }
    }

    /// The tight service profile in salvage mode.
    pub const fn service() -> Self {
        Self {
            mode: DecodeMode::Salvage,
            limits: ResourceLimits::service(),
        }
    }
}

impl Default for DecodePolicy {
    fn default() -> Self {
        Self::desktop()
    }
}

/// Resource options for container inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InspectOptions {
    /// The absolute ceilings applied during inspection.
    pub limits: ResourceLimits,
}

pub(super) const DECOMPRESSED_TOTAL_BASE: u64 = 16 * MIB;
pub(super) const DECOMPRESSED_TOTAL_PER_INPUT_BYTE: u64 = 1000;
pub(super) const DECOMPRESSED_PER_EXPAND_BASE: u64 = 16 * MIB;
pub(super) const DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE: u64 = 256;
pub(super) const MATERIALIZED_BASE: u64 = 16 * MIB;
pub(super) const MATERIALIZED_PER_INPUT_BYTE: u64 = 8000;
pub(super) const RETAINED_BASE: u64 = 16 * MIB;
pub(super) const RETAINED_PER_INPUT_BYTE: u64 = 8000;

const fn default_max_materialized_bytes() -> u64 {
    ResourceLimits::desktop().max_materialized_bytes
}

const fn default_max_retained_bytes() -> u64 {
    ResourceLimits::desktop().max_retained_bytes
}

const fn default_max_entities() -> u64 {
    ResourceLimits::desktop().max_entities
}

const fn default_max_collection_items() -> u64 {
    ResourceLimits::desktop().max_collection_items
}

const fn default_max_recursion_depth() -> u64 {
    ResourceLimits::desktop().max_recursion_depth
}

const fn default_max_work_units() -> u64 {
    ResourceLimits::desktop().max_work_units
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{ResourceLimits, GIB, MIB};

    #[test]
    fn desktop_ceilings_are_explicit() {
        let d = ResourceLimits::desktop();
        assert_eq!(d.max_input_bytes, 4 * GIB);
        assert_eq!(d.max_decompressed_bytes_total, 8 * GIB);
        assert_eq!(d.max_decompressed_bytes_per_expand, 2 * GIB);
        assert_eq!(d.max_materialized_bytes, 4 * GIB);
        assert_eq!(d.max_retained_bytes, 4 * GIB);
        assert_eq!(d.max_entities, 64_000_000);
        assert_eq!(d.max_collection_items, 16_000_000);
        assert_eq!(d.max_recursion_depth, 256);
        assert_eq!(d.max_work_units, 4_000_000_000);
    }

    #[test]
    fn service_ceilings_are_explicit() {
        let s = ResourceLimits::service();
        assert_eq!(s.max_input_bytes, 256 * MIB);
        assert_eq!(s.max_decompressed_bytes_total, GIB);
        assert_eq!(s.max_decompressed_bytes_per_expand, 256 * MIB);
        assert_eq!(s.max_materialized_bytes, 512 * MIB);
        assert_eq!(s.max_retained_bytes, 512 * MIB);
        assert_eq!(s.max_entities, 8_000_000);
        assert_eq!(s.max_collection_items, 1_000_000);
        assert_eq!(s.max_recursion_depth, 128);
        assert_eq!(s.max_work_units, 256_000_000);
    }

    #[test]
    fn added_limits_default_when_deserializing_old_policy() {
        let json = r#"{
            "max_input_bytes": 1,
            "max_decompressed_bytes_total": 2,
            "max_decompressed_bytes_per_expand": 3
        }"#;
        let limits: ResourceLimits = serde_json::from_str(json).unwrap();
        let desktop = ResourceLimits::desktop();
        assert_eq!(
            limits.max_materialized_bytes,
            desktop.max_materialized_bytes
        );
        assert_eq!(limits.max_retained_bytes, desktop.max_retained_bytes);
        assert_eq!(limits.max_entities, desktop.max_entities);
        assert_eq!(limits.max_collection_items, desktop.max_collection_items);
        assert_eq!(limits.max_recursion_depth, desktop.max_recursion_depth);
        assert_eq!(limits.max_work_units, desktop.max_work_units);
    }
}
