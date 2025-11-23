//! C++ bindings for Nix that expose the C++ API with C ABI.
//!
//! This crate provides thin C++ wrapper functions that allow Rust code to
//! access Nix C++ API features not available in the C API, such as allocating
//! values within primop callbacks.

#![no_std]

use core::ffi::c_char;

use nix_bindings_sys::{BindingsBuilder, EvalState, ListBuilder, Value};

// Attrsets.
unsafe extern "C" {
    /// Create a bindings builder with the specified capacity.
    ///
    /// This is what `nix_make_bindings_builder` SHOULD do, but it segfaults.
    pub fn make_bindings_builder(
        state: *mut EvalState,
        capacity: usize,
    ) -> *mut BindingsBuilder;

    /// Insert a key-value pair into the bindings builder.
    pub fn bindings_builder_insert(
        builder: *mut BindingsBuilder,
        name: *const c_char,
        value: *mut Value,
    );

    /// Finalize the bindings builder into an attribute set value.
    ///
    /// This frees the builder automatically.
    pub fn make_attrs(ret: *mut Value, builder: *mut BindingsBuilder);

    /// Get an attribute by name from an attribute set without forcing it.
    ///
    /// This is what `nix_get_attr_byname_lazy` SHOULD do, but it segfaults.
    ///
    /// Returns a pointer to the attribute's value if found, or null if the
    /// attribute doesn't exist.
    pub fn get_attr_byname_lazy(
        value: *const Value,
        state: *mut EvalState,
        name: *const c_char,
    ) -> *mut Value;
}

// Lists.
unsafe extern "C" {
    /// Create a list builder with the specified size.
    ///
    /// This is what `nix_make_list_builder` SHOULD do, but it segfaults.
    pub fn make_list_builder(
        state: *mut EvalState,
        size: usize,
    ) -> *mut ListBuilder;

    /// Insert a value at the given index in the list builder.
    pub fn list_builder_insert(
        builder: *mut ListBuilder,
        index: usize,
        value: *mut Value,
    );

    /// Finalize the list builder into a list value.
    ///
    /// This frees the builder automatically.
    pub fn make_list(ret: *mut Value, builder: *mut ListBuilder);
}

// Values.
unsafe extern "C" {
    /// Allocate a value using the C++ API.
    ///
    /// This is what `nix_alloc_value` SHOULD do, but it segfaults.
    ///
    /// Note: Values are managed by Nix's garbage collector (Boehm GC) and do
    /// NOT need to be explicitly freed.
    pub fn alloc_value(state: *mut EvalState) -> *mut Value;

    /// Force evaluation of a value using the C++ API.
    ///
    /// This is what `nix_value_force` SHOULD do, but it segfaults.
    pub fn force_value(state: *mut EvalState, value: *mut Value);
}
