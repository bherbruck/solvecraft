// SPDX-License-Identifier: Apache-2.0

use super::storage_cases;
use crate::decode::DecodeContext;
use crate::CodecError;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

storage_case!(
    collect_hash_map_storage,
    99,
    167,
    |ctx: &DecodeContext<'_>, count| {
        ctx.collect_hash_map(
            (0..count).map(|value| (u64::try_from(value).expect("small key"), 0u64)),
            "collected map storage",
        )
    }
);
storage_case!(
    push_hash_group_storage,
    171,
    227,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = HashMap::<u64, Vec<u64>>::new();
        for _ in 0..count {
            ctx.push_hash_group(&mut values, 0, 0, "group storage", "member storage")?;
        }
        Ok(values)
    }
);
// The first ordered group has one exact member slot. Its first growth selects
// four slots, then eight; the map has one 496-byte node admission.
storage_case!(
    push_btree_group_storage,
    504,
    560,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = BTreeMap::<u64, Vec<u64>>::new();
        for _ in 0..count {
            ctx.push_btree_group(&mut values, 0, 0, "tree group storage", "member storage")?;
        }
        Ok(values)
    }
);
// One group and one or five set members admit 496 + 232 backing bytes.
storage_case!(
    insert_btree_group_set_storage,
    728,
    728,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = BTreeMap::<u64, BTreeSet<u64>>::new();
        for value in 0..count {
            ctx.insert_btree_group_set(
                &mut values,
                0,
                u64::try_from(value).expect("small member"),
                "tree group storage",
                "set member storage",
            )?;
        }
        Ok(values)
    }
);
storage_case!(
    reserve_record_vec_storage,
    32,
    64,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = Vec::<u64>::new();
        for _ in 0..count {
            ctx.reserve_record_vec(&mut values, 1, 0, "record storage")?;
            values.push(0);
        }
        Ok(values)
    }
);
storage_case!(
    append_vec_storage,
    32,
    64,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = Vec::<u64>::new();
        for _ in 0..count {
            ctx.append_vec(&mut values, &mut vec![0], "appended storage")?;
        }
        Ok(values)
    }
);
storage_case!(
    extend_vec_storage,
    32,
    64,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = Vec::<u64>::new();
        for _ in 0..count {
            ctx.extend_vec(&mut values, vec![0], "extended storage")?;
        }
        Ok(values)
    }
);
storage_case!(
    collect_options_storage,
    32,
    64,
    |ctx: &DecodeContext<'_>, count| {
        ctx.collect_options(std::iter::repeat_n(Some(0u64), count), "optional storage")
    }
);
storage_case!(
    collect_fallible_options_storage,
    32,
    64,
    |ctx: &DecodeContext<'_>, count| {
        ctx.collect_fallible_options(
            (0..count).map(|_| Ok::<_, CodecError>(Some(0u64))),
            "fallible optional storage",
        )
    }
);
storage_case!(
    try_collect_retained_with_storage,
    8,
    40,
    |ctx: &DecodeContext<'_>, count| {
        ctx.try_collect_retained_with(0..count, "mapped storage", |_| Ok::<_, CodecError>(0u64))
    }
);
storage_case!(
    copy_retained_rows_storage,
    32,
    160,
    |ctx: &DecodeContext<'_>, count| {
        ctx.copy_retained_rows(
            &std::array::from_fn::<_, 5, _>(|_| vec![0u64])[..count],
            "row storage",
            "row member storage",
        )
    }
);
storage_case!(
    copy_rows_storage,
    32,
    160,
    |ctx: &DecodeContext<'_>, count| {
        ctx.copy_rows(
            &[0u64; 5][..count],
            1,
            "flat row storage",
            "row member storage",
        )
    }
);
storage_case!(
    copy_retained_set_storage,
    67,
    103,
    |ctx: &DecodeContext<'_>, count| {
        let values = (0..count)
            .map(|value| u64::try_from(value).expect("small value"))
            .collect();
        ctx.copy_retained_set(&values, "copied set storage")
    }
);
storage_case!(
    collect_retained_texts_storage,
    25,
    125,
    |ctx: &DecodeContext<'_>, count| {
        ctx.collect_retained_texts(std::iter::repeat_n("x", count), "copied text storage")
    }
);
storage_case!(
    insert_string_set_storage,
    132,
    236,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = HashSet::new();
        for value in &["a", "b", "c", "d", "e"][..count] {
            ctx.insert_string_set(&mut values, value, "copied set text storage")?;
        }
        Ok(values)
    }
);
storage_case!(
    collect_string_set_storage,
    132,
    236,
    |ctx: &DecodeContext<'_>, count| {
        ctx.collect_string_set(
            ["a", "b", "c", "d", "e"][..count].iter().copied(),
            "collected set text storage",
        )
    }
);
storage_case!(
    resize_retained_bytes_storage,
    8,
    8,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = Vec::new();
        ctx.resize_retained_bytes(&mut values, count, 0, "resized byte storage")?;
        Ok(values)
    }
);
storage_case!(
    reserve_vec_limit_storage,
    32,
    64,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = Vec::new();
        for _ in 0..count {
            ctx.reserve_vec_limit(&mut values, 1, "resource-only vector storage")?;
            values.push(0u64);
        }
        Ok(values)
    }
);
storage_case!(
    reserve_capacity_limit_storage,
    32,
    64,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = Vec::new();
        for _ in 0..count {
            ctx.reserve_capacity_limit(&mut values, 1, "resource-only backing storage")?;
            values.push(0u64);
        }
        Ok(values)
    }
);
// One or five map entries admit one 320-byte backing node.
storage_case!(
    admit_btree_node_storage,
    320,
    320,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = BTreeMap::new();
        for value in 0..count {
            ctx.admit_btree_node_storage::<u64, u64>(values.len(), "tree node storage")?;
            values.insert(u64::try_from(value).expect("small key"), 0u64);
        }
        Ok(values)
    }
);
// Each record admits one 320-byte node: one or five records cost 320 or 1600 bytes.
storage_case!(
    admit_retained_btree_record_storage,
    320,
    1600,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = BTreeMap::new();
        for value in 0..count {
            ctx.admit_retained_btree_record::<u64, u64>(0, "tree record storage")?;
            values.insert(u64::try_from(value).expect("small key"), 0u64);
        }
        Ok(values)
    }
);
storage_case!(
    exact_vec_storage,
    8,
    40,
    |ctx: &DecodeContext<'_>, count| {
        let view = crate::decode::View::over_retained(&[0u8; 5]);
        let bounded = view
            .counted(u64::try_from(count).expect("small count"), 1)
            .expect("bounded count");
        let mut values = super::super::super::ExactVec::new(ctx, bounded, "exact vector storage")?;
        for _ in 0..count {
            values.push(0u64)?;
        }
        values.finish()
    }
);
storage_case!(
    extend_retained_bytes_storage,
    8,
    8,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = Vec::new();
        ctx.extend_retained_bytes(&mut values, &[0u8; 5][..count], "extended bytes storage")?;
        Ok(values)
    }
);
storage_case!(
    retained_string_storage,
    1,
    5,
    |ctx: &DecodeContext<'_>, count| { ctx.retained_string(count, "retained string storage") }
);
storage_case!(
    retained_suffix_storage,
    1,
    5,
    |ctx: &DecodeContext<'_>, count| {
        ctx.retained_suffix("", &"xxxxx"[..count], "retained suffix storage")
    }
);
storage_case!(
    copy_retained_strings_storage,
    25,
    125,
    |ctx: &DecodeContext<'_>, count| {
        let values = std::array::from_fn::<_, 5, _>(|_| String::from("x"));
        ctx.copy_retained_strings(&values[..count], "retained string vector storage")
    }
);
storage_case!(
    join_retained_storage,
    1,
    5,
    |ctx: &DecodeContext<'_>, count| {
        ctx.join_retained(&["x"; 5][..count], "", "retained join storage")
    }
);
storage_case!(
    format_retained_storage,
    1,
    5,
    |ctx: &DecodeContext<'_>, count| {
        ctx.format_retained(
            format_args!("{}", &"xxxxx"[..count]),
            "retained format storage",
        )
    }
);
storage_case!(
    append_retained_storage,
    1,
    5,
    |ctx: &DecodeContext<'_>, count| {
        let mut value = String::new();
        for _ in 0..count {
            ctx.append_retained(&mut value, "x", "retained append storage")?;
        }
        Ok(value)
    }
);
storage_case!(
    append_formatted_retained_storage,
    1,
    5,
    |ctx: &DecodeContext<'_>, count| {
        let mut value = String::new();
        for _ in 0..count {
            ctx.append_formatted_retained(
                &mut value,
                format_args!("x"),
                "retained format append storage",
            )?;
        }
        Ok(value)
    }
);
storage_case!(
    push_formatted_retained_storage,
    97,
    197,
    |ctx: &DecodeContext<'_>, count| {
        let mut values = Vec::new();
        for _ in 0..count {
            ctx.push_formatted_retained(
                &mut values,
                format_args!("x"),
                "note slots",
                "note storage",
            )?;
        }
        Ok(values)
    }
);
storage_case!(
    charge_formatted_retained_storage,
    1,
    5,
    |ctx: &DecodeContext<'_>, count| {
        ctx.charge_formatted_retained(
            format_args!("{}", &"xxxxx"[..count]),
            "aggregate formatted storage",
        )?;
        Ok(())
    }
);
