// SPDX-License-Identifier: Apache-2.0
use super::super::{AnchorResolver, BTreeMap, Value};

#[test]
fn entity_index_is_not_part_of_exchange_equality() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POINT();ENDSEC;END-ISO-10303-21;";
    let (indexed, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("required invariant");
    let (untouched, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("required invariant");
    assert_eq!(indexed.entities("POINT").count(), 1);
    assert_eq!(indexed, untouched);
}

#[test]
fn released_source_graph_drops_records_and_cached_entity_indexes() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POINT();ENDSEC;END-ISO-10303-21;";
    let (mut exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("required invariant");
    assert!(exchange.has_entity("POINT"));

    let _ = exchange.release_source_graph();

    assert!(exchange.records().is_empty());
    assert!(exchange.header().is_empty());
    assert!(exchange.data().is_empty());
    assert!(!exchange.has_entity("POINT"));
}

#[test]
fn entity_unions_are_ordered_unique_and_name_order_independent() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#2=(A()B());#1=B();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("required invariant");

    let forward = exchange
        .entities_any(&["A", "B"])
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    let reverse = exchange
        .entities_any(&["B", "A"])
        .map(|(id, _)| id)
        .collect::<Vec<_>>();

    assert_eq!(forward, vec![1, 2]);
    assert_eq!(reverse, forward);
}

#[test]
fn entity_union_queries_remain_ordered_across_repeated_queries() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#3=C();#2=(A()B());#1=B();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid record graph");
    assert_eq!(
        exchange
            .entities_any(&["A", "B"])
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        vec![1, 2],
    );

    for names in [["B", "A"], ["B", "C"]] {
        let actual = exchange
            .entities_any(&names)
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let expected = exchange
            .records()
            .iter()
            .filter(|(_, record)| {
                record
                    .partials
                    .iter()
                    .any(|partial| names.contains(&partial.name.as_str()))
            })
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}

#[test]
fn anchor_budget_charges_only_resource_expansion() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::new();
        let mut resolver = AnchorResolver::new(&anchors, ctx).expect("empty resolver scope fits");
        resolver.remaining_nodes = 0;

        let ordinary = Value::List((0..1024).map(Value::Integer).collect());
        assert_eq!(
            resolver.resolve_root(&ordinary).expect("ordinary value"),
            ordinary
        );
        assert_eq!(resolver.remaining_nodes, 0);
    });
}

#[test]
fn anchor_budget_still_bounds_resource_materialization() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".to_string(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let mut resolver = AnchorResolver::new(&anchors, ctx).expect("empty resolver scope fits");
        resolver.remaining_nodes = 2;

        assert!(resolver
            .resolve_root(&Value::Resource("#a".to_string()))
            .is_err());
    });
}

#[test]
fn data_population_uses_source_extent_without_a_second_id_collection() {
    use cadmpeg_core::decode::{u64_from_index, DecodePolicy};
    use std::fmt::Write as _;

    let count = 10_000;
    let mut source = String::from("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;");
    for id in 1..=count {
        write!(source, "#{id}=ITEM();").expect("authored record");
    }
    source.push_str("ENDSEC;END-ISO-10303-21;");
    let mut policy = DecodePolicy::service();
    // Each record has one graph entry and one entity-index member. The
    // fixed allowance covers the header, schema, diagnostics and DATA node.
    policy.limits.max_collection_items = u64_from_index(count) * 2 + 128;
    crate::test_support::with_policy_context(source.as_bytes(), &policy, |source, ctx| {
        let (exchange, _) = crate::parse::parse_inner(source, ctx).expect("two record indexes fit");
        assert_eq!(exchange.records().len(), count);
        assert_eq!(exchange.data().len(), 1);
        assert!(exchange
            .records()
            .values()
            .all(|record| exchange.data()[0].span.contains(&record.span.start)));
    });
}
