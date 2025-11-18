use crate::Value;
use crate::value::AttrsetValue;

/// TODO: docs.
pub trait Attrset: Sized {
    /// TODO: docs.
    #[inline]
    fn into_value(self) -> impl Value {
        AttrsetValue(self)
    }
}
