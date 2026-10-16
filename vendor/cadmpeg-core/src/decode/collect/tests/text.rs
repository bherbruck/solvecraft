// SPDX-License-Identifier: Apache-2.0

use super::context;
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::collections::{HashMap, HashSet};
admitted_case!(
    reserve_admitted_vec_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx.reserve_capacity(&mut Vec::<u8>::new(), 2, "test admitted")
);
admitted_case!(
    reserve_admitted_map_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx.reserve_map(&mut HashMap::<u8, u8>::new(), 2, "test admitted")
);
admitted_case!(
    reserve_admitted_set_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx.reserve_set(&mut HashSet::<u8>::new(), 2, "test admitted")
);
admitted_case!(
    copy_admitted_slice_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx.copy_slice(&[1_u8, 2], "test admitted").map(|_| ())
);
admitted_case!(
    copy_admitted_rows_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx
        .copy_rows(&[1_u8, 2], 1, "test admitted", "test admitted")
        .map(|_| ())
);
admitted_case!(
    admitted_vec_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx.vector_storage::<u8>(2, "test admitted").map(|_| ())
);

#[test]
fn formatted_text_admits_growth_when_display_length_changes() {
    struct ChangingText(std::cell::Cell<bool>);
    impl std::fmt::Display for ChangingText {
        fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            output.write_str(if self.0.replace(true) { "abcd" } else { "a" })
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    let changing = ChangingText(std::cell::Cell::new(false));
    let error = ctx
        .format_retained(format_args!("{changing}"), "changing text")
        .expect_err("test operation refuses");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 1 && limit.additional == 3));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("admitted test operation");
    let changing = ChangingText(std::cell::Cell::new(false));
    assert_eq!(
        ctx.format_retained(format_args!("{changing}"), "changing text")
            .expect("admitted test operation"),
        "abcd"
    );
}

retained_case!(
    copy_retained_slice_charges_before_allocation,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .copy_slice(&[1_u8, 2], "test retained slice")
        .map(|_| ())
);
retained_case!(
    copy_retained_rows_charges_before_allocation,
    crate::decode::u64_from_index(std::mem::size_of::<Vec<u8>>()),
    |ctx: &DecodeContext<'_>| ctx
        .copy_retained_rows(
            &[vec![1_u8]],
            "test retained rows",
            "test retained row items"
        )
        .map(|_| ())
);
retained_case!(
    copy_retained_set_charges_before_allocation,
    crate::decode::u64_from_index(std::mem::size_of::<u8>() + 32),
    |ctx: &DecodeContext<'_>| ctx
        .copy_retained_set(&HashSet::from([1_u8]), "test retained set")
        .map(|_| ())
);
retained_case!(
    format_retained_charges_before_allocation,
    3,
    |ctx: &DecodeContext<'_>| ctx
        .format_retained(format_args!("abc"), "test format retained")
        .map(|_| ())
);
retained_case!(
    append_retained_charges_before_growth,
    3,
    |ctx: &DecodeContext<'_>| ctx.append_retained(
        &mut String::new(),
        "abc",
        "test append retained"
    )
);
retained_case!(
    retained_suffix_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx.retained_suffix("a", "b", "test suffix").map(|_| ())
);
retained_case!(
    join_retained_charges_before_allocation,
    3,
    |ctx: &DecodeContext<'_>| ctx
        .join_retained(&["a", "b"], "-", "test join retained")
        .map(|_| ())
);
retained_case!(
    join_display_retained_charges_before_growth,
    7,
    |ctx: &DecodeContext<'_>| ctx
        .join_display_retained(["one", "two"], ",", "test display join")
        .map(|_| ())
);
retained_case!(
    retained_string_charges_before_allocation,
    3,
    |ctx: &DecodeContext<'_>| ctx.retained_string(3, "test retained string").map(|_| ())
);
retained_case!(
    copy_retained_charges_before_allocation,
    3,
    |ctx: &DecodeContext<'_>| ctx
        .copy_retained(b"abc", "test optional retained")
        .map(|_| ())
);

materialized_case!(
    temporary_set_reserves_scoped_storage,
    33,
    |ctx: &DecodeContext<'_>| ctx.temporary_set::<u8>(1, "test temporary set").map(|_| ())
);
materialized_case!(
    temporary_queue_reserves_scoped_storage,
    1,
    |ctx: &DecodeContext<'_>| ctx
        .temporary_queue::<u8>(1, "test temporary queue")
        .map(|_| ())
);
materialized_case!(
    format_scoped_charges_before_allocation,
    3,
    |ctx: &DecodeContext<'_>| ctx
        .format_scoped(format_args!("abc"), "test format scoped")
        .map(|_| ())
);

#[test]
fn copy_scoped_text_refuses_before_allocation_and_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped text")
        .expect("empty reserve");
    let result = ctx.copy_scoped_text("abc", &mut reservation, "test scoped text");
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped text")
        .expect("empty reserve");
    assert_eq!(
        ctx.copy_scoped_text("abc", &mut reservation, "test scoped text")
            .expect("copy"),
        "abc"
    );
}
#[test]
fn charged_join_refuses_input_sized_text_before_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 5;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        ctx.join_display_retained(["one", "two"], ",", "step_test_join"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_test_join"
    ));
}

#[test]
fn charged_format_refuses_retained_text_before_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let error = ctx
        .format_retained(
            format_args!("prefix {suffix}", suffix = "input"),
            "step_test_format",
        )
        .expect_err("formatted text exceeds three bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "step_test_format"
    ));
}
