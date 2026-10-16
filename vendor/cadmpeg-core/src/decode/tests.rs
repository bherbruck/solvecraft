// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use std::io::{self, Cursor, Read, Seek, SeekFrom};

use crate::CodecError;

use super::u64_from_index;
use super::{
    refuse_local_limit, ByteRange, DecodeArena, DecodeContext, DecodePolicy, ExpandSpec,
    ResourceDimension, ResourceLimits, WorkBudget,
};

fn policy_with(mut edit: impl FnMut(&mut ResourceLimits)) -> DecodePolicy {
    let mut policy = DecodePolicy::default();
    edit(&mut policy.limits);
    policy
}

#[test]
fn copy_retained_text_refuses_before_allocation_and_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_retained_bytes = 4);
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert!(matches!(
        ctx.copy_retained_text("hello", "retained text test"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default()).unwrap();
    assert_eq!(
        ctx.copy_retained_text("hello", "retained text test")
            .unwrap(),
        "hello"
    );
}

#[test]
fn allocation_failed_constructor_sets_refusal_fields() {
    let limit =
        super::ResourceLimit::allocation_failed(ResourceDimension::Codec("test"), 12, 5, "test");
    assert_eq!(limit.dimension, ResourceDimension::Codec("test"));
    assert_eq!(limit.reason, super::ResourceFailure::AllocationFailed);
    assert_eq!(limit.limit, 12);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 5);
    assert_eq!(limit.operation, "test");
}

#[test]
fn root_limit_is_enforced() {
    let bytes = [0_u8; 5];
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_input_bytes = 4);
    assert!(matches!(
        DecodeContext::from_root_bytes(&bytes, &arena, &policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::InputBytes
    ));
}

#[test]
fn read_root_uses_sized_and_fallback_read_paths() {
    let bytes = vec![0_u8; 32];
    let policy = policy_with(|limits| limits.max_input_bytes = u64_from_index(bytes.len()));

    let arena = DecodeArena::new();
    let mut seekable = Cursor::new(bytes.clone());
    let (_, root) = DecodeContext::read_root(&mut seekable, &arena, &policy, false).unwrap();
    assert_eq!(root.window(), bytes.as_slice());

    let arena = DecodeArena::new();
    let mut fallback = Unseekable::new(bytes.clone());
    let (_, root) = DecodeContext::read_root(&mut fallback, &arena, &policy, false).unwrap();
    assert_eq!(root.window(), bytes.as_slice());
}

struct Unseekable {
    input: Cursor<Vec<u8>>,
}

impl Unseekable {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            input: Cursor::new(bytes),
        }
    }
}

impl Read for Unseekable {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.input.read(bytes)
    }
}

impl Seek for Unseekable {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::other("seek unavailable"))
    }
}

#[test]
fn views_bound_reads_and_children() {
    let bytes = [0, 1, 2, 3, 4];
    let arena = DecodeArena::new();
    let (_, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let mut child = root.child(1, 4).unwrap();
    assert_eq!(child.take(3), Some(&bytes[1..4]));
    assert!(child.take(1).is_none());
    assert!(root.child(3, 6).is_none());
}

#[test]
fn counted_requires_a_physically_possible_count() {
    let bytes = [0_u8; 8];
    let arena = DecodeArena::new();
    let (_, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    assert!(root.counted(3, 4).is_none());
    assert_eq!(root.counted(2, 4).unwrap().get(), 2);
}

#[test]
fn exact_expansion_enforces_size_and_limit() {
    let bytes = [0_u8; 4];
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| {
        limits.max_decompressed_bytes_total = 8;
        limits.max_decompressed_bytes_per_expand = 8;
    });
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut writer = ctx.begin_expand(ExpandSpec::Exact(4)).unwrap();
    writer.write(&[1, 2, 3, 4]).unwrap();
    let view = writer.finalize().unwrap();
    assert_eq!(view.window(), &[1, 2, 3, 4]);

    let mut writer = ctx.begin_expand(ExpandSpec::Unknown).unwrap();
    assert!(matches!(
        writer.write(&[0; 9]),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn concatenation_and_stored_slices_have_distinct_spaces() {
    let bytes = [0, 1, 2, 3, 4, 5];
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let first = root.child(0, 2).unwrap();
    let second = root.child(4, 6).unwrap();
    assert!(matches!(
        ctx.concat_views(&[]),
        Err(CodecError::Malformed(_))
    ));
    let concat = ctx.concat_views(&[first, second]).unwrap();
    assert_eq!(concat.window(), &[0, 1, 4, 5]);
    let slice = ctx
        .register_slice(root, ByteRange { start: 1, end: 3 })
        .unwrap();
    assert_eq!(slice.window(), &[1, 2]);
    assert_ne!(concat.space(), slice.space());
}

#[test]
fn scoped_reservations_release_and_commit_without_double_counting() {
    let bytes = [0_u8; 1];
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| {
        limits.max_materialized_bytes = 5;
        limits.max_retained_bytes = 7;
    });
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    {
        let mut reservation = ctx.reserve_scoped(3, "temporary").unwrap();
        reservation.grow(2).unwrap();
    }
    ctx.reserve_scoped(5, "released").unwrap();
    let reservation = ctx.reserve_scoped(2, "retained").unwrap();
    reservation.commit().unwrap();
    ctx.charge_retained(5, "retained").unwrap();
}

#[test]
fn lossy_utf8_copy_charges_replacement_bytes_before_retention() {
    let source = b"A\xffB\xe2\x82";
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_retained_bytes = 7);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = ctx
        .copy_retained_lossy_utf8(source, "lossy UTF-8 fixture")
        .expect_err("two replacements need eight bytes");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "lossy UTF-8 fixture"));

    let policy = policy_with(|limits| limits.max_retained_bytes = 8);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        ctx.copy_retained_lossy_utf8(source, "lossy UTF-8 fixture")
            .expect("eight retained bytes fit"),
        String::from_utf8_lossy(source)
    );
    assert_eq!(
        ctx.copy_retained_lossy_utf8(&[], "empty UTF-8 fixture")
            .expect("empty text needs no bytes"),
        ""
    );
}

#[test]
fn every_session_dimension_refuses_and_fuses() {
    fn assert_dimension(
        edit: impl FnMut(&mut ResourceLimits),
        charge: impl FnOnce(&DecodeContext<'_>) -> Result<(), CodecError>,
        expected: ResourceDimension,
    ) {
        let arena = DecodeArena::new();
        let policy = policy_with(edit);
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
        assert!(
            matches!(charge(&ctx), Err(CodecError::ResourceLimit(limit)) if limit.dimension == expected)
        );
        assert!(matches!(
            ctx.charge_entities(0, "after_fuse"),
            Err(CodecError::ResourceLimit(_))
        ));
    }

    assert_dimension(
        |limits| limits.max_materialized_bytes = 1,
        |ctx| ctx.reserve_scoped(2, "materialize").map(drop),
        ResourceDimension::MaterializedBytes,
    );
    assert_dimension(
        |limits| limits.max_retained_bytes = 1,
        |ctx| ctx.charge_retained(2, "retain"),
        ResourceDimension::RetainedBytes,
    );
    assert_dimension(
        |limits| limits.max_entities = 1,
        |ctx| ctx.charge_entities(2, "entities"),
        ResourceDimension::Entities,
    );
    assert_dimension(
        |limits| limits.max_entities = 1,
        |ctx| {
            let mut admitted = 0;
            ctx.admit_entities(1, &mut admitted, "admit")?;
            ctx.admit_entities(2, &mut admitted, "admit")
        },
        ResourceDimension::Entities,
    );
    assert_dimension(
        |limits| limits.max_collection_items = 1,
        |ctx| ctx.charge_collection_items(2, "items"),
        ResourceDimension::CollectionItems,
    );
    assert_dimension(
        |limits| limits.max_recursion_depth = 0,
        |ctx| ctx.enter_nested("nested").map(drop),
        ResourceDimension::RecursionDepth,
    );
    assert_dimension(
        |limits| limits.max_work_units = 1,
        |ctx| ctx.charge_work(2, "work"),
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn tiny_resource_limits_refuse_entities_and_materialized_bytes() {
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_entities = 1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
    assert!(matches!(
        ctx.charge_entities(2, "entities"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
    ));

    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_materialized_bytes = 1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
    assert!(matches!(
        ctx.reserve_scoped(2, "materialize"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn alloc_filled_charges_collection_items_and_reserves() {
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_collection_items = 1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
    assert!(matches!(
        ctx.alloc_filled(2, 0u8, "filled"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
    ));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default()).unwrap();
    assert_eq!(ctx.alloc_filled(3, 7u8, "filled").unwrap(), vec![7, 7, 7]);
}

#[test]
fn alloc_filled_admitted_charges_items_and_work_but_no_bytes() {
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| {
        limits.max_retained_bytes = 0;
        limits.max_materialized_bytes = 0;
        limits.max_collection_items = 3;
        limits.max_work_units = 3;
    });
    let ctx = DecodeContext::new(&arena, &policy, false);
    assert_eq!(
        ctx.alloc_filled_admitted(3, 7u64, "admitted").unwrap(),
        [7, 7, 7]
    );
    assert!(matches!(
        ctx.alloc_filled_admitted(1, 0u8, "admitted"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
    ));
    let policy = policy_with(|limits| limits.max_collection_items = 1);
    let ctx = DecodeContext::new(&arena, &policy, false);
    assert!(matches!(
        ctx.alloc_filled_admitted(2, 0u8, "admitted"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admitted"
    ));
}

#[test]
fn depth_is_scoped_and_work_budget_is_sticky() {
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| {
        limits.max_recursion_depth = 1;
        limits.max_work_units = 3;
    });
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
    {
        let _guard = ctx.enter_nested("first").unwrap();
    }
    ctx.enter_nested("second").unwrap();

    let budget = ctx.work_budget(10);
    assert!(budget.charge_by(2));
    let child = budget.child_slice(1);
    assert!(child.charge());
    assert!(matches!(budget.consume_child(&child), Ok(())));
    assert_eq!(budget.consumed(), 3);
    assert!(!budget.charge());
    assert!(budget.exhausted());
    assert!(!budget.charge_by(0));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits)
    );
}

#[test]
fn local_limit_refusal_uses_codec_dimension() {
    assert!(matches!(
        refuse_local_limit("records", 4, 5),
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("records")
                && limit.operation == "records"
    ));
}

#[test]
fn codec_limit_refusal_fuses_the_decode_session() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default()).unwrap();
    assert!(matches!(
        ctx.refuse_codec_limit("nested_records", 4, 5),
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("nested_records")
                && limit.limit == 4
                && limit.used == 4
                && limit.additional == 1
    ));
    assert!(matches!(
        ctx.charge_work(0, "after_codec_limit"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Codec("nested_records")
    ));
}

#[test]
fn committed_reads_preserve_truncation_location_and_operation() {
    let arena = DecodeArena::new();
    let (_, mut root) =
        DecodeContext::from_root_bytes(&[1, 2], &arena, &DecodePolicy::default()).unwrap();
    let error: CodecError = root
        .req_u32_le()
        .unwrap_err()
        .during("read header size")
        .into();
    assert!(matches!(
        error,
        CodecError::Truncated {
            location,
            operation
        }
            if location == root.location_at(0)
                && operation == "read header size"
    ));
}

#[test]
fn forced_child_exhaustion_charges_its_full_slice() {
    let parent = WorkBudget::new(10);
    let child = parent.child_slice(4);
    assert!(child.charge());
    child.exhaust();
    assert!(matches!(parent.consume_child(&child), Ok(())));
    assert_eq!(parent.remaining(), 6);
    assert!(!child.charge_by(0));
}

#[test]
fn stored_expanded_and_concatenated_spaces_have_distinct_zero_based_locations() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(b"abcd", &arena, &policy).unwrap();
    let stored = ctx
        .register_slice(root, ByteRange { start: 0, end: 4 })
        .unwrap();
    let expanded = ctx
        .begin_expand(ExpandSpec::Exact(0))
        .unwrap()
        .finalize()
        .unwrap();
    let concat = ctx.concat_views(&[root, stored]).unwrap();
    for (index, view) in [root, stored, expanded, concat].into_iter().enumerate() {
        assert_eq!(view.space().index(), index);
        assert_eq!(view.location().offset, 0);
    }
}

#[test]
fn filled_storage_and_fill_work_are_admitted_before_cloning() {
    use std::cell::Cell;
    struct Counted<'a>(&'a Cell<usize>);
    impl Clone for Counted<'_> {
        fn clone(&self) -> Self {
            self.0.set(self.0.get() + 1);
            Self(self.0)
        }
    }
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::RetainedBytes,
        ResourceDimension::MaterializedBytes,
    ] {
        let arena = DecodeArena::new();
        let policy = policy_with(|limits| match dimension {
            ResourceDimension::WorkUnits => limits.max_work_units = 0,
            ResourceDimension::RetainedBytes => limits.max_retained_bytes = 0,
            ResourceDimension::MaterializedBytes => limits.max_materialized_bytes = 0,
            _ => unreachable!(),
        });
        let ctx = DecodeContext::new(&arena, &policy, false);
        let clones = Cell::new(0);
        let run = || ctx.alloc_filled(3, Counted(&clones), "filled accounting");
        let result = if dimension == ResourceDimension::MaterializedBytes {
            ctx.with_scoped_storage("filled backing", run)
                .map(|(values, _)| values)
        } else {
            run()
        };
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == dimension && limit.operation == "filled accounting"));
        assert_eq!(clones.get(), 0);
    }
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| {
        limits.max_work_units = 3;
        limits.max_retained_bytes = 24;
    });
    let ctx = DecodeContext::new(&arena, &policy, false);
    assert_eq!(
        ctx.alloc_filled(3, 7u64, "filled accounting").unwrap(),
        [7, 7, 7]
    );
    assert_eq!(ctx.charge_work_limit(1, "measure").unwrap_err().used, 3);
}

#[test]
fn lossy_utf8_scans_and_copy_refuse_without_work() {
    for source in [b"valid".as_slice(), b"A\xffB\xe2\x82".as_slice()] {
        let arena = DecodeArena::new();
        let policy = policy_with(|limits| limits.max_work_units = 0);
        let ctx = DecodeContext::new(&arena, &policy, false);
        assert!(matches!(ctx.copy_retained_lossy_utf8(source, "UTF-8 work"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits));
    }
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_work_units = 15);
    let ctx = DecodeContext::new(&arena, &policy, false);
    assert_eq!(
        ctx.copy_retained_lossy_utf8(b"valid", "UTF-8 work")
            .unwrap(),
        "valid"
    );
    assert_eq!(ctx.charge_work_limit(1, "measure").unwrap_err().used, 15);
}

#[test]
fn concatenation_admits_extent_scan_and_byte_copy_separately() {
    for (budget, requested) in [(0, 2), (2, 4)] {
        let arena = DecodeArena::new();
        let policy = policy_with(|limits| limits.max_work_units = budget);
        let ctx = DecodeContext::new(&arena, &policy, false);
        let views = [
            super::View::over_retained(b"ab"),
            super::View::over_retained(b"cd"),
        ];
        assert!(
            matches!(ctx.concat_views(&views), Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits && limit.additional == requested)
        );
    }
    let arena = DecodeArena::new();
    let policy = policy_with(|limits| limits.max_work_units = 6);
    let ctx = DecodeContext::new(&arena, &policy, false);
    assert_eq!(
        ctx.concat_views(&[
            super::View::over_retained(b"ab"),
            super::View::over_retained(b"cd")
        ])
        .unwrap()
        .window(),
        b"abcd"
    );
    assert_eq!(ctx.charge_work_limit(1, "measure").unwrap_err().used, 6);
}
