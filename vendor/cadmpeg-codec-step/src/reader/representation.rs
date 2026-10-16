// SPDX-License-Identifier: Apache-2.0
//! Shared access to inherited `REPRESENTATION` attributes.

use super::ValueExt;
use crate::parse::{RawRecord, Value};

pub(super) fn parameters(record: &RawRecord) -> Option<&[Value]> {
    record.partials.iter().find_map(|partial| {
        (is_representation_name(&partial.name) && !partial.parameters.is_empty())
            .then_some(partial.parameters.as_slice())
    })
}

pub(super) fn items(record: &RawRecord) -> Option<impl DoubleEndedIterator<Item = u64> + '_> {
    item_values(record).map(|items| items.iter().filter_map(ValueExt::reference))
}

pub(super) fn item_values(record: &RawRecord) -> Option<&[Value]> {
    record.partials.iter().find_map(|partial| {
        if !is_representation_name(&partial.name) {
            return None;
        }
        partial.parameters.get(1).and_then(ValueExt::list)
    })
}

pub(super) fn is_representation_name(name: &str) -> bool {
    name == "REPRESENTATION"
        || name.ends_with("_REPRESENTATION")
        || name == "SHAPE_REPRESENTATION_WITH_PARAMETERS"
        || name == "TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS"
}

#[cfg(test)]
mod tests {
    use super::{items, parameters};
    use crate::parse::Value;

    #[test]
    fn shape_representation_with_parameters_uses_inherited_attributes() {
        let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SHAPE_REPRESENTATION_WITH_PARAMETERS('datum target',(#2,#3),#4);#2=ITEM();#3=ITEM();#4=ITEM();ENDSEC;END-ISO-10303-21;";
        let (exchange, _) =
            crate::test_support::with_service_context(source, crate::parse::parse_inner)
                .expect("representation attributes");
        let record = &exchange.records()[&1];

        assert_eq!(
            parameters(record),
            Some(
                [
                    Value::String(b"datum target".to_vec()),
                    Value::List(vec![Value::Reference(2), Value::Reference(3)]),
                    Value::Reference(4),
                ]
                .as_slice()
            )
        );
        assert_eq!(
            items(record).map(Iterator::collect::<Vec<_>>),
            Some(vec![2, 3])
        );
    }
}
