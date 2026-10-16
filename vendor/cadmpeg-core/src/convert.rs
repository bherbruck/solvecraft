// SPDX-License-Identifier: Apache-2.0
//! Checked conversions between numeric representations.

#![expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "each conversion checks exactness or the finite target range before casting"
)]

/// Converts zero or an index with at most 53 significant bits after removing trailing zeros.
pub fn f64_from_index(value: usize) -> Option<f64> {
    if value == 0 || usize::BITS - value.leading_zeros() - value.trailing_zeros() <= 53 {
        Some(value as f64)
    } else {
        None
    }
}

/// Converts zero or an unsigned integer with at most 53 significant bits after removing trailing zeros.
pub fn f64_from_u64(value: u64) -> Option<f64> {
    if value == 0 || u64::BITS - value.leading_zeros() - value.trailing_zeros() <= 53 {
        Some(value as f64)
    } else {
        None
    }
}

/// Converts zero or a signed integer whose magnitude has at most 53 significant bits
/// after removing trailing zeros. This includes `i64::MIN`.
pub fn f64_from_i64(value: i64) -> Option<f64> {
    let magnitude = value.unsigned_abs();
    if magnitude == 0 || u64::BITS - magnitude.leading_zeros() - magnitude.trailing_zeros() <= 53 {
        Some(value as f64)
    } else {
        None
    }
}

/// Rounds a finite `f64` to the nearest representable `f32`.
pub fn f32_from_f64(value: f64) -> Option<f32> {
    if value.is_finite() && value.abs() <= f64::from(f32::MAX) {
        Some(value as f32)
    } else {
        None
    }
}

/// Truncates a finite `f64` to an `i32` when the result fits.
pub fn truncate_f64_to_i32(value: f64) -> Option<i32> {
    let value = value.trunc();
    if value.is_finite() && value >= f64::from(i32::MIN) && value < 2_147_483_648.0 {
        Some(value as i32)
    } else {
        None
    }
}

/// Truncates a finite `f64` to an `i64` when the result fits.
pub fn truncate_f64_to_i64(value: f64) -> Option<i64> {
    let value = value.trunc();
    if value.is_finite()
        && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value)
    {
        Some(value as i64)
    } else {
        None
    }
}

/// Truncates a finite `f64` to an `i128` when the result fits.
pub fn truncate_f64_to_i128(value: f64) -> Option<i128> {
    let value = value.trunc();
    if value.is_finite()
        && (-170_141_183_460_469_231_731_687_303_715_884_105_728.0
            ..170_141_183_460_469_231_731_687_303_715_884_105_728.0)
            .contains(&value)
    {
        Some(value as i128)
    } else {
        None
    }
}

/// Truncates a finite `f64` to a `u8` when the result fits.
pub fn truncate_f64_to_u8(value: f64) -> Option<u8> {
    let value = value.trunc();
    if value.is_finite() && (0.0..256.0).contains(&value) {
        Some(value as u8)
    } else {
        None
    }
}

/// Truncates a finite `f64` to a `u16` when the result fits.
pub fn truncate_f64_to_u16(value: f64) -> Option<u16> {
    let value = value.trunc();
    if value.is_finite() && (0.0..65_536.0).contains(&value) {
        Some(value as u16)
    } else {
        None
    }
}

/// Truncates a finite `f64` to a `u32` when the result fits.
pub fn truncate_f64_to_u32(value: f64) -> Option<u32> {
    let value = value.trunc();
    if value.is_finite() && (0.0..4_294_967_296.0).contains(&value) {
        Some(value as u32)
    } else {
        None
    }
}

/// Truncates a finite `f64` to a `u64` when the result fits.
pub fn truncate_f64_to_u64(value: f64) -> Option<u64> {
    let value = value.trunc();
    if value.is_finite() && (0.0..18_446_744_073_709_551_616.0).contains(&value) {
        Some(value as u64)
    } else {
        None
    }
}

/// Truncates a finite `f64` to a `usize` when the result fits.
pub fn truncate_f64_to_usize(value: f64) -> Option<usize> {
    let value = value.trunc();
    let upper = if usize::BITS == 64 {
        18_446_744_073_709_551_616.0
    } else {
        4_294_967_296.0
    };
    if value.is_finite() && value >= 0.0 && value < upper {
        Some(value as usize)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f64_from_index_checks_exact_boundary() {
        assert_eq!(f64_from_index(0), Some(0.0));
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(
                f64_from_index(9_007_199_254_740_992),
                Some(9_007_199_254_740_992.0)
            );
            assert_eq!(f64_from_index(9_007_199_254_740_993), None);
            assert_eq!(
                f64_from_index(9_007_199_254_740_994),
                Some(9_007_199_254_740_994.0)
            );
            assert_eq!(f64_from_index(usize::MAX), None);
            assert_eq!(
                f64_from_index(1_usize << 63),
                Some(9_223_372_036_854_775_808.0)
            );
        }
        #[cfg(target_pointer_width = "32")]
        assert_eq!(f64_from_index(usize::MAX), Some(4_294_967_295.0));
    }

    #[test]
    fn f64_from_u64_checks_exact_boundary() {
        assert_eq!(f64_from_u64(0), Some(0.0));
        assert_eq!(f64_from_u64(u64::MAX), None);
        assert_eq!(f64_from_u64(1_u64 << 63), Some(9_223_372_036_854_775_808.0));
        assert_eq!(
            f64_from_u64(9_007_199_254_740_994),
            Some(9_007_199_254_740_994.0)
        );
        assert_eq!(
            f64_from_u64(9_007_199_254_740_992),
            Some(9_007_199_254_740_992.0)
        );
        assert_eq!(f64_from_u64(9_007_199_254_740_993), None);
    }

    #[test]
    fn f64_from_i64_checks_both_exact_boundaries() {
        assert_eq!(f64_from_i64(0), Some(0.0));
        assert_eq!(f64_from_i64(i64::MIN), Some(-9_223_372_036_854_775_808.0));
        assert_eq!(f64_from_i64(i64::MAX), None);
        assert_eq!(
            f64_from_i64(9_007_199_254_740_994),
            Some(9_007_199_254_740_994.0)
        );
        assert_eq!(
            f64_from_i64(-9_007_199_254_740_994),
            Some(-9_007_199_254_740_994.0)
        );
        assert_eq!(
            f64_from_i64(-9_007_199_254_740_992),
            Some(-9_007_199_254_740_992.0)
        );
        assert_eq!(f64_from_i64(-9_007_199_254_740_993), None);
        assert_eq!(
            f64_from_i64(9_007_199_254_740_992),
            Some(9_007_199_254_740_992.0)
        );
        assert_eq!(f64_from_i64(9_007_199_254_740_993), None);
    }

    #[test]
    fn f32_from_f64_checks_range_and_finiteness() {
        assert_eq!(f32_from_f64(f64::from(f32::MAX)), Some(f32::MAX));
        assert_eq!(f32_from_f64(f64::from(f32::MAX) * 2.0), None);
        assert_eq!(f32_from_f64(f64::from(f32::MAX).next_up()), None);
        assert_eq!(f32_from_f64(-f64::from(f32::MAX)), Some(-f32::MAX));
        assert_eq!(f32_from_f64((-f64::from(f32::MAX)).next_down()), None);
        assert_eq!(f32_from_f64(-1.0), Some(-1.0));
        assert_eq!(f32_from_f64(f64::NAN), None);
        assert_eq!(f32_from_f64(f64::INFINITY), None);
        assert_eq!(f32_from_f64(f64::NEG_INFINITY), None);
    }

    #[test]
    fn truncate_f64_to_i32_checks_bounds_and_finiteness() {
        assert_eq!(truncate_f64_to_i32(-2_147_483_648.9), Some(i32::MIN));
        assert_eq!(truncate_f64_to_i32(-2_147_483_649.0), None);
        assert_eq!(truncate_f64_to_i32(2_147_483_647.9), Some(i32::MAX));
        assert_eq!(truncate_f64_to_i32(2_147_483_648.0), None);
        assert_eq!(truncate_f64_to_i32(f64::NAN), None);
        assert_eq!(truncate_f64_to_i32(f64::INFINITY), None);
        assert_eq!(truncate_f64_to_i32(f64::NEG_INFINITY), None);
    }

    #[test]
    fn truncate_f64_to_i64_checks_bounds_and_finiteness() {
        assert_eq!(
            truncate_f64_to_i64(-9_223_372_036_854_775_808.0),
            Some(i64::MIN)
        );
        assert_eq!(truncate_f64_to_i64(-9_223_372_036_854_777_856.0), None);
        assert_eq!(truncate_f64_to_i64(9_223_372_036_854_775_808.0), None);
        assert_eq!(
            truncate_f64_to_i64(9_223_372_036_854_774_784.0),
            Some(9_223_372_036_854_774_784)
        );
        assert_eq!(truncate_f64_to_i64(f64::INFINITY), None);
        assert_eq!(truncate_f64_to_i64(f64::NAN), None);
        assert_eq!(truncate_f64_to_i64(f64::NEG_INFINITY), None);
    }

    #[test]
    fn truncate_f64_to_i128_checks_bounds_and_finiteness() {
        let upper = 170_141_183_460_469_231_731_687_303_715_884_105_728.0;
        assert_eq!(truncate_f64_to_i128(-upper), Some(i128::MIN));
        assert_eq!(truncate_f64_to_i128(upper), None);
        assert_eq!(
            truncate_f64_to_i128(f64::from_bits(upper.to_bits() - 1)),
            Some(i128::MAX - (1_i128 << 74) + 1)
        );
        assert_eq!(truncate_f64_to_i128(f64::NAN), None);
        assert_eq!(truncate_f64_to_i128(f64::INFINITY), None);
        assert_eq!(truncate_f64_to_i128(f64::NEG_INFINITY), None);
    }

    #[test]
    fn truncate_f64_to_u8_checks_bounds_and_finiteness() {
        assert_eq!(truncate_f64_to_u8(-0.9), Some(0));
        assert_eq!(truncate_f64_to_u8(-1.0), None);
        assert_eq!(truncate_f64_to_u8(255.9), Some(u8::MAX));
        assert_eq!(truncate_f64_to_u8(256.0), None);
        assert_eq!(truncate_f64_to_u8(f64::NAN), None);
        assert_eq!(truncate_f64_to_u8(f64::INFINITY), None);
        assert_eq!(truncate_f64_to_u8(f64::NEG_INFINITY), None);
    }

    #[test]
    fn truncate_f64_to_u16_checks_bounds_and_finiteness() {
        assert_eq!(truncate_f64_to_u16(-1.0), None);
        assert_eq!(truncate_f64_to_u16(65_535.9), Some(u16::MAX));
        assert_eq!(truncate_f64_to_u16(65_536.0), None);
        assert_eq!(truncate_f64_to_u16(f64::NAN), None);
        assert_eq!(truncate_f64_to_u16(f64::NEG_INFINITY), None);
        assert_eq!(truncate_f64_to_u16(f64::INFINITY), None);
    }

    #[test]
    fn truncate_f64_to_u32_checks_bounds_and_finiteness() {
        assert_eq!(truncate_f64_to_u32(-1.0), None);
        assert_eq!(truncate_f64_to_u32(4_294_967_295.9), Some(u32::MAX));
        assert_eq!(truncate_f64_to_u32(4_294_967_296.0), None);
        assert_eq!(truncate_f64_to_u32(f64::NAN), None);
        assert_eq!(truncate_f64_to_u32(f64::INFINITY), None);
        assert_eq!(truncate_f64_to_u32(f64::NEG_INFINITY), None);
    }

    #[test]
    fn truncate_f64_to_u64_checks_bounds_and_finiteness() {
        assert_eq!(truncate_f64_to_u64(-1.0), None);
        assert_eq!(
            truncate_f64_to_u64(18_446_744_073_709_547_520.0),
            Some(18_446_744_073_709_547_520)
        );
        assert_eq!(truncate_f64_to_u64(18_446_744_073_709_551_616.0), None);
        assert_eq!(
            truncate_f64_to_u64(18_446_744_073_709_549_568.0),
            Some(18_446_744_073_709_549_568)
        );
        assert_eq!(truncate_f64_to_u64(f64::NAN), None);
        assert_eq!(truncate_f64_to_u64(f64::NEG_INFINITY), None);
        assert_eq!(truncate_f64_to_u64(f64::INFINITY), None);
    }

    #[test]
    fn truncate_f64_to_usize_checks_bounds_and_finiteness() {
        let upper = if usize::BITS == 64 {
            18_446_744_073_709_551_616.0
        } else {
            4_294_967_296.0
        };
        assert_eq!(truncate_f64_to_usize(-1.0), None);
        assert_eq!(truncate_f64_to_usize(upper), None);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            truncate_f64_to_usize(18_446_744_073_709_549_568.0),
            Some(18_446_744_073_709_549_568)
        );
        #[cfg(target_pointer_width = "32")]
        assert_eq!(truncate_f64_to_usize(4_294_967_295.0), Some(4_294_967_295));
        assert_eq!(truncate_f64_to_usize(f64::NEG_INFINITY), None);
        assert_eq!(truncate_f64_to_usize(f64::NAN), None);
        assert_eq!(truncate_f64_to_usize(f64::INFINITY), None);
        assert_eq!(truncate_f64_to_usize(1.9), Some(1));
    }
}
