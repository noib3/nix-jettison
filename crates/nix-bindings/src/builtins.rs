//! TODO: docs.

use crate::attrset::NixAttrset;
use crate::context::Context;
use crate::prelude::NixLambda;

/// TODO: docs.
pub struct Builtins<'eval> {
    inner: NixAttrset<'eval>,
}

impl<'eval> Builtins<'eval> {
    /// Returns a handle to the `builtins.fetchGit` function.
    #[inline]
    pub fn fetch_git(&self, ctx: &mut Context) -> NixLambda<'eval> {
        self.inner
            .get(c"fetchGit", ctx)
            .expect("builtins.fetchGit exists and it's a function")
    }

    #[inline]
    pub(crate) fn new(inner: NixAttrset<'eval>) -> Self {
        Self { inner }
    }
}
