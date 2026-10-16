// SPDX-License-Identifier: Apache-2.0

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

fn scoped_storage_cases<T>(
    small: u64,
    grown: u64,
    build: impl for<'ctx> Fn(
        &'ctx DecodeContext<'ctx>,
        usize,
    ) -> Result<(T, crate::decode::ScopedReservation<'ctx>), CodecError>,
) {
    for (count, bytes) in [(1, small), (5, grown)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = bytes;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
        let (values, reservation) = build(&ctx, count).expect("exact scoped storage");
        let error = ctx
            .reserve_scoped(1, "verify scoped wrapper charge")
            .expect_err("all scoped bytes used");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes && limit.used == bytes
                && ctx.resource_refusal() == Some(limit)));
        drop(values);
        drop(reservation);

        let arena = DecodeArena::new();
        policy.limits.max_materialized_bytes = bytes - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
        let Err(error) = build(&ctx, count) else {
            panic!("one below the scoped charge must refuse");
        };
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes && ctx.resource_refusal() == Some(limit)));

        let arena = DecodeArena::new();
        policy.limits.max_materialized_bytes = bytes;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
        let (values, reservation) = build(&ctx, count).expect("unfused scoped storage");
        drop(values);
        drop(reservation);
        ctx.reserve_scoped(bytes, "released scoped wrapper storage")
            .expect("scope released backing");
    }
}

macro_rules! scoped_storage_case {
    ($name:ident, $small:expr, $grown:expr, $value:ty, $ctx:ident, $count:ident, $body:block) => {
        #[test]
        fn $name() {
            fn build<'ctx>($ctx: &'ctx DecodeContext<'ctx>, $count: usize) -> Result<($value, crate::decode::ScopedReservation<'ctx>), CodecError> $body
            scoped_storage_cases($small, $grown, build);
        }
    };
}

scoped_storage_case!(temporary_vec_storage, 8, 40, Vec<u64>, ctx, count, {
    ctx.temporary_vec(count, "temporary vector storage")
});
scoped_storage_case!(scoped_vector_storage_bytes, 8, 40, Vec<u64>, ctx, count, {
    ctx.scoped_vector_storage(count, "scoped vector backing")
});
scoped_storage_case!(
    reserve_temporary_vec_storage,
    8,
    40,
    Vec<u64>,
    ctx,
    count,
    {
        let mut values = Vec::new();
        let reservation =
            ctx.reserve_temporary_vec(&mut values, count, "temporary reserve storage")?;
        Ok((values, reservation))
    }
);
scoped_storage_case!(copy_temporary_slice_storage, 8, 40, Vec<u64>, ctx, count, {
    Ok(ctx.copy_temporary_slice(&[0u64; 5][..count], "temporary copied storage")?)
});
scoped_storage_case!(reserve_scoped_vec_storage, 32, 64, Vec<u64>, ctx, count, {
    let mut reservation = ctx.reserve_scoped(0, "scoped growth storage")?;
    let mut values = Vec::new();
    for _ in 0..count {
        let capacity = values.capacity();
        let result =
            ctx.reserve_scoped_vec(&mut reservation, &mut values, 1, "scoped growth storage");
        if result.is_err() {
            assert_eq!(values.capacity(), capacity);
        }
        result?;
        values.push(0);
    }
    Ok((values, reservation))
});
scoped_storage_case!(push_scoped_vec_storage, 32, 64, Vec<u64>, ctx, count, {
    let mut reservation = ctx.reserve_scoped(0, "scoped push storage")?;
    let mut values = Vec::new();
    for _ in 0..count {
        let capacity = values.capacity();
        let result = ctx.push_scoped_vec(&mut reservation, &mut values, 0, "scoped push storage");
        if result.is_err() {
            assert_eq!(values.capacity(), capacity);
        }
        result?;
    }
    Ok((values, reservation))
});
scoped_storage_case!(temporary_set_storage, 67, 103, HashSet<u64>, ctx, count, {
    ctx.temporary_set(count, "temporary set storage")
});
scoped_storage_case!(
    temporary_set_limit_storage,
    67,
    103,
    HashSet<u64>,
    ctx,
    count,
    { Ok(ctx.temporary_set_limit(count, "temporary resource-only set storage")?) }
);
scoped_storage_case!(temporary_queue_storage, 8, 40, VecDeque<u64>, ctx, count, {
    ctx.temporary_queue(count, "temporary queue storage")
});
scoped_storage_case!(
    collect_scoped_texts_storage,
    25,
    125,
    Vec<String>,
    ctx,
    count,
    {
        ctx.collect_scoped_texts(
            std::iter::repeat_n("x", count),
            "scoped copied text storage",
        )
    }
);
scoped_storage_case!(copy_scoped_text_storage, 1, 5, String, ctx, count, {
    let mut reservation = ctx.reserve_scoped(0, "scoped copied text storage")?;
    let text = ctx.copy_scoped_text(
        &"xxxxx"[..count],
        &mut reservation,
        "scoped copied text storage",
    )?;
    Ok((text, reservation))
});
scoped_storage_case!(scoped_string_storage, 1, 5, String, ctx, count, {
    ctx.scoped_string(count, "scoped string storage")
});
scoped_storage_case!(reserve_scoped_string_storage, 1, 5, String, ctx, count, {
    let mut reservation = ctx.reserve_scoped(0, "scoped text growth storage")?;
    let mut text = String::new();
    for _ in 0..count {
        let capacity = text.capacity();
        let result =
            ctx.reserve_scoped_string(&mut reservation, &mut text, 1, "scoped text growth storage");
        if result.is_err() {
            assert_eq!(text.capacity(), capacity);
        }
        result?;
        text.push('x');
    }
    Ok((text, reservation))
});
scoped_storage_case!(format_scoped_storage, 1, 5, String, ctx, count, {
    ctx.format_scoped(
        format_args!("{}", &"xxxxx"[..count]),
        "scoped formatted storage",
    )
});
scoped_storage_case!(format_scoped_text_storage, 1, 5, String, ctx, count, {
    let mut reservation = ctx.reserve_scoped(0, "scoped formatted text storage")?;
    let text = ctx.format_scoped_text(
        &mut reservation,
        format_args!("{}", &"xxxxx"[..count]),
        "scoped formatted text storage",
    )?;
    Ok((text, reservation))
});
// One or five scoped set entries admit one 232-byte backing node.
scoped_storage_case!(
    insert_scoped_btree_set_storage,
    232,
    232,
    BTreeSet<u64>,
    ctx,
    count,
    {
        let mut reservation = ctx.reserve_scoped(0, "scoped tree storage")?;
        let mut values = BTreeSet::new();
        for value in 0..count {
            ctx.insert_scoped_btree_set(
                &mut reservation,
                &mut values,
                u64::try_from(value).expect("small value"),
                "tree work",
                "scoped tree storage",
            )?;
        }
        Ok((values, reservation))
    }
);
// One or five scoped values admit one 232-byte backing node.
scoped_storage_case!(
    insert_scoped_btree_value_storage,
    232,
    232,
    BTreeSet<u64>,
    ctx,
    count,
    {
        let mut reservation = ctx.reserve_scoped(0, "scoped tree value storage")?;
        let mut values = BTreeSet::new();
        for value in 0..count {
            ctx.insert_scoped_btree_value(
                &mut reservation,
                &mut values,
                u64::try_from(value).expect("small value"),
                "scoped tree value storage",
            )?;
        }
        Ok((values, reservation))
    }
);
// One or five scoped map entries admit one 320-byte backing node.
scoped_storage_case!(insert_scoped_btree_map_if_vacant_storage, 320, 320, BTreeMap<u64, u64>, ctx, count, {
    let mut reservation = ctx.reserve_scoped(0, "scoped tree map storage")?;
    let mut values = BTreeMap::new();
    for value in 0..count { ctx.insert_scoped_btree_map_if_vacant(&mut reservation, &mut values, u64::try_from(value).expect("small key"), 0, "tree work", "scoped tree map storage")?; }
    Ok((values, reservation))
});
scoped_storage_case!(push_scoped_btree_group_storage, 504, 560, BTreeMap<u64, Vec<u64>>, ctx, count, {
    let mut reservation = ctx.reserve_scoped(0, "scoped tree group storage")?;
    let mut values = BTreeMap::new();
    for _ in 0..count { ctx.push_scoped_btree_group(&mut reservation, &mut values, 0, || 0, 0, "scoped tree group storage")?; }
    Ok((values, reservation))
});
scoped_storage_case!(collect_scoped_btree_groups_storage, 504, 560, BTreeMap<u64, Vec<u64>>, ctx, count, {
    ctx.collect_scoped_btree_groups(std::iter::repeat_n((0, 0), count), "scoped collected group storage")
});
// One or five collected scoped entries admit one 320-byte backing node.
scoped_storage_case!(collect_scoped_btree_map_storage, 320, 320, BTreeMap<u64, u64>, ctx, count, {
    ctx.collect_scoped_btree_map((0..count).map(|value| (u64::try_from(value).expect("small key"), 0)), "scoped collected map storage")
});
scoped_storage_case!(
    collect_scoped_string_set_storage,
    99,
    167,
    HashSet<&'static str>,
    ctx,
    count,
    {
        ctx.collect_scoped_string_set(
            count,
            ["a", "b", "c", "d", "e"][..count].iter().copied(),
            "scoped borrowed set storage",
        )
    }
);
scoped_storage_case!(
    collect_scoped_string_map_storage,
    131,
    231,
    HashMap<&'static str, u64>,
    ctx,
    count,
    {
        ctx.collect_scoped_string_map(
            count,
            ["a", "b", "c", "d", "e"][..count]
                .iter()
                .copied()
                .map(|key| (key, 0)),
            "scoped borrowed map storage",
        )
    }
);

scoped_storage_case!(
    reserve_scoped_vec_limit_storage,
    32,
    64,
    Vec<u64>,
    ctx,
    count,
    {
        let mut reservation = ctx.reserve_scoped(0, "resource-only scoped vector")?;
        let mut values = Vec::new();
        for _ in 0..count {
            ctx.reserve_scoped_vec_limit(
                &mut reservation,
                &mut values,
                1,
                "resource-only scoped vector",
            )?;
            values.push(0);
        }
        Ok((values, reservation))
    }
);
scoped_storage_case!(reserve_scoped_collection_storage, 8, 40, (), ctx, count, {
    let reservation =
        ctx.reserve_scoped_collection::<u64>(count, "scoped collection reservation")?;
    Ok(((), reservation))
});
