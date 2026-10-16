// SPDX-License-Identifier: Apache-2.0
//! Absent-only reads for optional wire keys.
//!
//! An `Option<T>` field written with `skip_serializing_if = "Option::is_none"`
//! writes nothing for `None`, so absence is the one spelling the writer
//! produces. Serde's own `Option` read also admits `null`, which gives the same
//! state a second spelling. [`named_present`] closes that: the key, when
//! stated, states a value, and the refusal names the key it refuses.
//!
//! A field declares that reader through [`crate::named_optional_field!`], which is the
//! only path to the refusal: the visitor behind it is private, so a field
//! cannot refuse `null` without stating which key it is refusing.
//!
//! A field with no `skip_serializing_if` is the mirror case: the writer always
//! states the key, so `null` is its spelling of `None` and an absent key is no
//! spelling at all. [`nullable`] reads that key, and the reading declaration
//! states no `default`, so serde names the field when it is left out.

use serde::Deserialize;

/// Read a stated optional key. `null` is refused; absence is `None` through the
/// field's `default`.
///
/// Private: the refusal it states names no key, so every declaration reaches it
/// through [`named_present`], which adds the key.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Visitor<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
        type Value = Option<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a stated value, or the key left out")
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Err(E::custom(
                "this key states a value or is left out; it does not state null",
            ))
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Err(E::custom(
                "this key states a value or is left out; it does not state null",
            ))
        }

        fn visit_some<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            T::deserialize(deserializer).map(Some)
        }
    }

    deserializer.deserialize_option(Visitor(std::marker::PhantomData))
}

/// Read a stated optional key and name the field in whatever it refuses.
///
/// The refusal itself sees a value, never a key: serde hands a `deserialize_with`
/// function the field's value deserializer and nothing else. The key travels
/// as an argument instead, so the refusal states the instance it refuses. A
/// flattened reader is the case that settles the shape: serde buffers a
/// flattened field's keys into its own content map before the reader runs, so
/// no path a surrounding deserializer tracks reaches inside one.
///
/// # Errors
///
/// Returns the `null` refusal with the key name before it.
pub fn named_present<'de, D, T>(deserializer: D, field: &str) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    present(deserializer)
        .map_err(|error| serde::de::Error::custom(format_args!("{field}: {error}")))
}

/// Define the named reader one optional key is declared with.
///
/// The expansion is the concrete function serde's field-deserializer path
/// requires, forwarding to [`named_present`] with the key it names. A field
/// whose type mentions its container's type parameters names them after the
/// reader: `named_optional_field!(deserialize_side<S>, Support<S>, "side")`. A
/// file that declares its keys in a child module states the visibility the
/// parent needs: `named_optional_field!(pub(super) deserialize_side, S, "side")`.
///
/// ```
/// cadmpeg_core::named_optional_field!(deserialize_label, String, "label");
///
/// #[derive(Debug, serde::Deserialize)]
/// struct Wire {
///     #[serde(default, deserialize_with = "deserialize_label")]
///     label: Option<String>,
/// }
///
/// let refusal = serde_json::from_str::<Wire>(r#"{"label": null}"#).unwrap_err();
/// assert!(refusal.to_string().starts_with("label: this key states a value"));
/// ```
#[macro_export]
macro_rules! named_optional_field {
    ($vis:vis $name:ident, $value:ty, $field:literal) => {
        $vis fn $name<'de, D: ::serde::Deserializer<'de>>(
            deserializer: D,
        ) -> ::core::result::Result<::core::option::Option<$value>, D::Error> {
            $crate::absent_key::named_present(deserializer, $field)
        }
    };
    ($vis:vis $name:ident<$($parameter:ident),+ $(,)?>, $value:ty, $field:literal) => {
        $vis fn $name<'de, D, $($parameter),+>(
            deserializer: D,
        ) -> ::core::result::Result<::core::option::Option<$value>, D::Error>
        where
            D: ::serde::Deserializer<'de>,
            $value: ::serde::Deserialize<'de>,
        {
            $crate::absent_key::named_present(deserializer, $field)
        }
    };
}

/// Read a required optional key whose writer spells `None` as `null`.
///
/// A field with no `skip_serializing_if` writes the key for every value, so
/// `null` is that key's own spelling of `None`. The reading declaration states
/// no `default`, so an absent key is a missing field named in the error and
/// `null` is the one spelling of `None`.
///
/// The helper exists so every optional key on a read type states which
/// spelling its writer produces: [`named_present`] where the writer omits the key,
/// this where the writer always states it. A field that states neither is an
/// undeclared key, and the wire-crate census names it.
///
/// # Errors
///
/// Returns the inner type's own error.
pub fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
