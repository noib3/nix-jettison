//! C++ bindings for Nix that expose the C++ API with C ABI.
//!
//! This crate provides thin C++ wrapper functions that allow Rust code to
//! access Nix C++ API features not available in the C API, such as allocating
//! values within primop callbacks.

#![no_std]

use core::ffi::{c_char, c_void};

use nix_bindings_c::{EvalState, Value};

// Attrsets.
unsafe extern "C" {
    /// Create a bindings builder with the specified capacity.
    ///
    /// This is what `nix_make_bindings_builder` SHOULD do but doesn't work in
    /// primop callbacks.
    pub fn make_bindings_builder(
        state: *mut EvalState,
        capacity: usize,
    ) -> *mut c_void;

    /// Insert a symbol-value pair into the bindings builder.
    pub fn bindings_builder_insert(
        builder: *mut c_void,
        symbol: *mut c_void,
        value: *mut Value,
    );

    /// Finalize the bindings builder into an attribute set value.
    ///
    /// This frees the builder automatically.
    pub fn make_attrs(ret: *mut Value, builder: *mut c_void);
}

// Symbols.
unsafe extern "C" {
    /// Create a symbol from a name.
    ///
    /// The returned pointer must be freed with `free_symbol`.
    pub fn create_symbol(
        state: *mut EvalState,
        name: *const c_char,
    ) -> *mut c_void;

    /// Free a symbol allocated by `create_symbol`.
    pub fn free_symbol(symbol: *mut c_void);
}

// Values.
unsafe extern "C" {
    /// Allocate a value using the C++ API.
    ///
    /// This is what `nix_alloc_value` SHOULD do but doesn't work in primop
    /// callbacks.
    pub fn alloc_value(state: *mut EvalState) -> *mut Value;
}
