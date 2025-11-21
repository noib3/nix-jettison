use alloc::borrow::Cow;
use alloc::ffi::CString;
use core::ffi::CStr;

use crate::Utf8CStr;

/// TODO: docs.
pub trait Namespace: Copy {
    #[doc(hidden)]
    fn push(self, name: &CStr) -> impl PoppableNamespace<Self>;

    #[doc(hidden)]
    fn display(self) -> Cow<'static, CStr>;
}

/// The type of namespace returned by [`Namespace::push`].
pub trait PoppableNamespace<Orig: Namespace>: Namespace {
    /// Removes the last name added by [`Namespace::push`], returning the
    /// original namespace.
    fn pop(self) -> Orig;
}

/// A [`Namespace`] implementation that concatenates two namespaces by adding
/// a `.` between them.
#[derive(Copy, Clone)]
struct ConcatNamespace<Left, Right>(Left, Right);

impl Namespace for &'static Utf8CStr {
    #[inline]
    fn display(self) -> Cow<'static, CStr> {
        Cow::Borrowed(self.as_c_str())
    }

    #[inline]
    fn push(self, name: &CStr) -> impl PoppableNamespace<Self> {
        ConcatNamespace(self, name)
    }
}

impl Namespace for &Cow<'static, CStr> {
    #[inline]
    fn display(self) -> Cow<'static, CStr> {
        self.clone()
    }

    #[inline]
    fn push(self, name: &CStr) -> impl PoppableNamespace<Self> {
        ConcatNamespace(self, name)
    }
}

impl<L: Namespace, R: AsRef<CStr> + Copy> Namespace for ConcatNamespace<L, R> {
    #[inline]
    fn display(self) -> Cow<'static, CStr> {
        let Self(left, right) = self;
        let mut vec = left.display().into_owned().into_bytes();
        vec.push(b'.');
        vec.extend_from_slice(right.as_ref().to_bytes());
        // SAFETY: neither CString::into_bytes() nor CStr::to_bytes() include
        // the terminating NUL byte, so the resulting vector is guaranteed to
        // not contain any interior NUL bytes.
        Cow::Owned(unsafe { CString::from_vec_unchecked(vec) })
    }

    #[inline]
    fn push(self, name: &CStr) -> impl PoppableNamespace<Self> {
        ConcatNamespace(self, name)
    }
}

impl<L: Namespace, R> PoppableNamespace<L> for ConcatNamespace<L, R>
where
    Self: Namespace,
{
    #[inline]
    fn pop(self) -> L {
        let Self(left, _) = self;
        left
    }
}
