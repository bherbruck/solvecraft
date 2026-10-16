// SPDX-License-Identifier: Apache-2.0

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

pub(super) fn context(arena: &DecodeArena, items: u64) -> DecodeContext<'_> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    match DecodeContext::from_root_bytes(&[], arena, &policy) {
        Ok((ctx, _)) => ctx,
        Err(error) => panic!("test context failed: {error}"),
    }
}

pub(super) fn operation_context(
    arena: &DecodeArena,
    dimension: ResourceDimension,
    limit: u64,
) -> DecodeContext<'_> {
    let mut policy = DecodePolicy::service();
    match dimension {
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = limit,
        ResourceDimension::Entities => policy.limits.max_entities = limit,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
        _ => panic!("test needs a byte, entity or work dimension"),
    }
    DecodeContext::from_root_bytes(&[], arena, &policy)
        .expect("empty root fits policy")
        .0
}

macro_rules! collection_case {
        ($name:ident, $needed:expr, $refused:expr, $success:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let ctx = context(&arena, $needed - 1);
                let result: Result<(), CodecError> = ($refused)(&ctx);
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems));
                let arena = DecodeArena::new();
                let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
                let result: Result<(), CodecError> = ($success)(&ctx);
                assert!(result.is_ok(), "service profile admits the operation");
            }
        };
    }

macro_rules! admitted_case {
        ($name:ident, $body:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let ctx = context(&arena, 1);
                let result: Result<(), CodecError> = (|| {
                    ctx.charge_collection_items(2, "test admitted")?;
                    ($body)(&ctx)
                })();
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems));
                let arena = DecodeArena::new();
                let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
                ctx.charge_collection_items(2, "test admitted").expect("admission");
                assert!(($body)(&ctx).is_ok());
            }
        };
    }

macro_rules! retained_case {
        ($name:ident, $need:expr, $body:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = $need - 1;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("test context");
                let result: Result<(), CodecError> = ($body)(&ctx);
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes));
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
                    .expect("test context");
                assert!(($body)(&ctx).is_ok());
            }
        };
    }

macro_rules! materialized_case {
        ($name:ident, $need:expr, $body:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = $need - 1;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("test context");
                let result: Result<(), CodecError> = ($body)(&ctx);
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::MaterializedBytes));
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
                    .expect("test context");
                assert!(($body)(&ctx).is_ok());
            }
        };
    }

macro_rules! operation_case {
        ($name:ident, $success:ident, $dimension:expr, $needed:expr, $operation:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let ctx = operation_context(&arena, $dimension, $needed - 1);
                let result: Result<(), CodecError> = ($operation)(&ctx);
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == $dimension));
            }
            #[test]
            fn $success() {
                let arena = DecodeArena::new();
                let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
                let result: Result<(), CodecError> = ($operation)(&ctx);
                assert!(result.is_ok(), "service profile admits operation");
            }
        };
    }

mod capacity;
mod collections;
mod groups;
mod operations;
mod storage;
mod text;
