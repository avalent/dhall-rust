#![doc(html_root_url = "https://docs.rs/serde_dhall/0.4.0")]
//! [Dhall][dhall] is a programmable configuration language that provides a non-repetitive
//! alternative to JSON and YAML.
//!
//! You can think of Dhall as: JSON + types + imports + functions
//!
//! For a description of the Dhall language, examples, tutorials, and more, see the [language
//! website][dhall].
//!
//! This crate provides support for consuming Dhall files the same way you would consume JSON or
//! YAML. It uses the [Serde][serde] serialization library to provide drop-in support for Dhall
//! for any datatype that supports serde (and that's a lot of them !).
//!
//! This library is limited to deserializing (reading) Dhall values; serializing (writing)
//! values to Dhall is not supported.
//!
//! # Basic usage
//!
//! The main entrypoint of this library is the [`from_str`][from_str] function. It reads a string
//! containing a Dhall expression and deserializes it into any serde-compatible type.
//!
//! This could mean a common Rust type like `HashMap`:
//!
//! ```rust
//! # fn main() -> serde_dhall::de::Result<()> {
//! use std::collections::HashMap;
//!
//! // Some Dhall data
//! let data = "{ x = 1, y = 1 + 1 } : { x: Natural, y: Natural }";
//!
//! // Deserialize it to a Rust type.
//! let deserialized_map: HashMap<String, usize> = serde_dhall::from_str(data)?;
//!
//! let mut expected_map = HashMap::new();
//! expected_map.insert("x".to_string(), 1);
//! expected_map.insert("y".to_string(), 2);
//!
//! assert_eq!(deserialized_map, expected_map);
//! # Ok(())
//! # }
//! ```
//!
//! or a custom datatype, using serde's `derive` mechanism:
//!
//! ```rust
//! # fn main() -> serde_dhall::de::Result<()> {
//! use serde::Deserialize;
//!
//! #[derive(Debug, Deserialize)]
//! struct Point {
//!     x: u64,
//!     y: u64,
//! }
//!
//! // Some Dhall data
//! let data = "{ x = 1, y = 1 + 1 } : { x: Natural, y: Natural }";
//!
//! // Convert the Dhall string to a Point.
//! let point: Point = serde_dhall::from_str(data)?;
//! assert_eq!(point.x, 1);
//! assert_eq!(point.y, 2);
//!
//! # Ok(())
//! # }
//! ```
//!
//! # Type correspondence
//!
//! The following Dhall types correspond to the following Rust types:
//!
//! Dhall  | Rust
//! -------|------
//! `Bool`  | `bool`
//! `Natural`  | `u64`, `u32`, ...
//! `Integer`  | `i64`, `i32`, ...
//! `Double`  | `f64`, `f32`, ...
//! `Text`  | `String`
//! `List T`  | `Vec<T>`
//! `Optional T`  | `Option<T>`
//! `{ x: T, y: U }`  | structs
//! `{ _1: T, _2: U }`  | `(T, U)`, structs
//! `{ x: T, y: T }`  | `HashMap<String, T>`, structs
//! `< x: T \| y: U >`  | enums
//! `T -> U`  | unsupported
//! `Prelude.JSON.Type`  | unsupported
//! `Prelude.Map.Type T U`  | unsupported
//!
//!
//! # Replacing `serde_json` or `serde_yaml`
//!
//! If you used to consume JSON or YAML, you only need to replace [serde_json::from_str] or
//! [serde_yaml::from_str] with [serde_dhall::from_str][from_str].
//!
//! [serde_json::from_str]: https://docs.serde.rs/serde_json/de/fn.from_str.html
//! [serde_yaml::from_str]: https://docs.serde.rs/serde_yaml/fn.from_str.html
//!
//!
//! # Additional Dhall typechecking
//!
//! When deserializing, normal type checking is done to ensure that the returned value is a valid
//! Dhall value, and that it can be deserialized into the required Rust type. However types are
//! first-class in Dhall, and this library allows you to additionally check that some input data
//! matches a given Dhall type. That way, a type error will be caught on the Dhall side, and have
//! pretty and explicit errors that point to the source file.
//!
//! There are two ways to typecheck a Dhall value: you can provide the type as Dhall text or you
//! can let Rust infer it for you.
//!
//! To provide a type written in Dhall, first parse it into a [`serde_dhall::Type`][Type], then
//! pass it to [`from_str_check_type`][from_str_check_type].
//!
//! ```rust
//! # fn main() -> serde_dhall::de::Result<()> {
//! use serde_dhall::Type;
//! use std::collections::HashMap;
//!
//! // Parse a Dhall type
//! let point_type_str = "{ x: Natural, y: Natural }";
//! let point_type: Type = serde_dhall::from_str(point_type_str)?;
//!
//! // Some Dhall data
//! let point_data = "{ x = 1, y = 1 + 1 }";
//!
//! // Deserialize the data to a Rust type. This checks that
//! // the data matches the provided type.
//! let deserialized_map: HashMap<String, usize> =
//!         serde_dhall::from_str_check_type(point_data, &point_type)?;
//!
//! let mut expected_map = HashMap::new();
//! expected_map.insert("x".to_string(), 1);
//! expected_map.insert("y".to_string(), 2);
//!
//! assert_eq!(deserialized_map, expected_map);
//! # Ok(())
//! # }
//! ```
//!
//! You can also let Rust infer the appropriate Dhall type, using the [StaticType] trait.
//!
//! ```rust
//! # fn main() -> serde_dhall::de::Result<()> {
//! use serde::Deserialize;
//! use serde_dhall::StaticType;
//!
//! #[derive(Debug, Deserialize, StaticType)]
//! struct Point {
//!     x: u64,
//!     y: u64,
//! }
//!
//! // Some Dhall data
//! let data = "{ x = 1, y = 1 + 1 }";
//!
//! // Convert the Dhall string to a Point.
//! let point: Point = serde_dhall::from_str_auto_type(data)?;
//! assert_eq!(point.x, 1);
//! assert_eq!(point.y, 2);
//!
//! // Invalid data fails the type validation
//! let invalid_data = "{ x = 1, z = 0.3 }";
//! assert!(serde_dhall::from_str_auto_type::<Point>(invalid_data).is_err());
//! # Ok(())
//! # }
//! ```
//!
//! [dhall]: https://dhall-lang.org/
//! [serde]: https://docs.serde.rs/serde/
//! [serde::Deserialize]: https://docs.serde.rs/serde/trait.Deserialize.html

mod serde;
mod static_type;

#[doc(inline)]
pub use de::{from_str, from_str_auto_type, from_str_check_type};
#[doc(hidden)]
pub use dhall_proc_macros::StaticType;
pub use static_type::StaticType;
#[doc(inline)]
pub use ty::Type;
#[doc(inline)]
pub use value::Value;

// A Dhall value.
#[doc(hidden)]
pub mod value {
    use dhall::SimpleValue;

    use super::de::Error;

    /// A Dhall value. This is a wrapper around [`dhall::SimpleValue`].
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Value(SimpleValue);

    impl Value {
        pub fn into_simple_value(self) -> SimpleValue {
            self.0
        }
    }

    impl super::de::sealed::Sealed for Value {}

    impl super::de::Deserialize for Value {
        fn from_dhall(v: &dhall::Value) -> super::de::Result<Self> {
            let sval = v.to_simple_value().ok_or_else(|| {
                Error::Deserialize(format!(
                    "this cannot be deserialized into a simple type: {}",
                    v
                ))
            })?;
            Ok(Value(sval))
        }
    }
}

// A Dhall type.
#[doc(hidden)]
pub mod ty {
    use dhall::{STyKind, SimpleType};

    use super::de::Error;

    /// A Dhall type. This is a wrapper around [`dhall::SimpleType`].
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Type(SimpleType);

    impl Type {
        pub fn into_simple_type(self) -> SimpleType {
            self.0
        }
        pub fn to_dhall_value(&self) -> dhall::Value {
            self.0.to_value()
        }

        pub(crate) fn from_simple_type(ty: SimpleType) -> Self {
            Type(ty)
        }
        pub(crate) fn from_stykind(k: STyKind) -> Self {
            Type(SimpleType::new(k))
        }
        pub(crate) fn make_optional_type(t: Type) -> Self {
            Type::from_stykind(STyKind::Optional(t.0))
        }
        pub(crate) fn make_list_type(t: Type) -> Self {
            Type::from_stykind(STyKind::List(t.0))
        }
        // Made public for the StaticType derive macro
        #[doc(hidden)]
        pub fn make_record_type(
            kts: impl Iterator<Item = (String, Type)>,
        ) -> Self {
            Type::from_stykind(STyKind::Record(
                kts.map(|(k, t)| (k, t.0)).collect(),
            ))
        }
        #[doc(hidden)]
        pub fn make_union_type(
            kts: impl Iterator<Item = (String, Option<Type>)>,
        ) -> Self {
            Type::from_stykind(STyKind::Union(
                kts.map(|(k, t)| (k, t.map(|t| t.0))).collect(),
            ))
        }
    }

    impl super::de::sealed::Sealed for Type {}

    impl super::de::Deserialize for Type {
        fn from_dhall(v: &dhall::Value) -> super::de::Result<Self> {
            let sty = v.to_simple_type().ok_or_else(|| {
                Error::Deserialize(format!(
                    "this cannot be deserialized into a simple type: {}",
                    v
                ))
            })?;
            Ok(Type(sty))
        }
    }
}

/// Deserialize Dhall data to a Rust data structure.
pub mod de {
    use super::StaticType;
    use super::Type;
    pub use error::{Error, Result};

    mod error {
        use dhall::error::Error as DhallError;

        pub type Result<T> = std::result::Result<T, Error>;

        #[derive(Debug)]
        #[non_exhaustive]
        pub enum Error {
            Dhall(DhallError),
            Deserialize(String),
        }

        impl std::fmt::Display for Error {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                match self {
                    Error::Dhall(err) => write!(f, "{}", err),
                    Error::Deserialize(err) => write!(f, "{}", err),
                }
            }
        }

        impl std::error::Error for Error {}

        impl serde::de::Error for Error {
            fn custom<T>(msg: T) -> Self
            where
                T: std::fmt::Display,
            {
                Error::Deserialize(msg.to_string())
            }
        }
    }

    pub(crate) mod sealed {
        pub trait Sealed {}
    }

    /// A data structure that can be deserialized from a Dhall expression
    ///
    /// This is automatically implemented for any type that [serde][serde]
    /// can deserialize.
    ///
    /// This trait cannot be implemented manually.
    pub trait Deserialize: sealed::Sealed + Sized {
        /// See [serde_dhall::from_str][crate::from_str]
        fn from_dhall(v: &dhall::Value) -> Result<Self>;
    }

    fn from_str_with_annot<T>(s: &str, ty: Option<&Type>) -> Result<T>
    where
        T: Deserialize,
    {
        let ty = ty.map(|ty| ty.to_dhall_value());
        let val = dhall::Value::from_str_with_annot(s, ty.as_ref())
            .map_err(Error::Dhall)?;
        T::from_dhall(&val)
    }

    /// Deserialize an instance of type `T` from a string of Dhall text.
    ///
    /// This will recursively resolve all imports in the expression, and
    /// typecheck it before deserialization. Relative imports will be resolved relative to the
    /// provided file. More control over this process is not yet available
    /// but will be in a coming version of this crate.
    pub fn from_str<T>(s: &str) -> Result<T>
    where
        T: Deserialize,
    {
        from_str_with_annot(s, None)
    }

    /// Deserialize an instance of type `T` from a string of Dhall text,
    /// additionally checking that it matches the supplied type.
    ///
    /// Like [from_str], but this additionally checks that
    /// the type of the provided expression matches the supplied type.
    pub fn from_str_check_type<T>(s: &str, ty: &Type) -> Result<T>
    where
        T: Deserialize,
    {
        from_str_with_annot(s, Some(ty))
    }

    /// Deserialize an instance of type `T` from a string of Dhall text,
    /// additionally checking that it matches the type of `T`.
    ///
    /// Like [from_str], but this additionally checks that
    /// the type of the provided expression matches the output type `T`. The [StaticType] trait
    /// captures Rust types that are valid Dhall types.
    pub fn from_str_auto_type<T>(s: &str) -> Result<T>
    where
        T: Deserialize + StaticType,
    {
        from_str_check_type(s, &<T as StaticType>::static_type())
    }
}
