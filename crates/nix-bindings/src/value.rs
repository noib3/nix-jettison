/// TODO: docs.
pub trait Value: Sealed {}

use sealed::Sealed;

mod sealed {
    pub trait Sealed {}
}
