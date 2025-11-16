use core::ffi::{c_char, c_void};
use core::ptr;

use {nix_bindings_cpp as cpp, nix_bindings_sys as sys};

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "C" fn double(
    _user_data: *mut c_void,
    ctx: *mut sys::c_context,
    _state: *mut sys::EvalState,
    args: *mut *mut sys::Value,
    ret: *mut sys::Value,
) {
    let n = sys::get_int(ctx, *args.offset(0));
    sys::init_int(ctx, ret, n * 2);
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "C" fn jettison_lib(
    _user_data: *mut c_void,
    ctx: *mut sys::c_context,
    state: *mut sys::EvalState,
    _args: *mut *mut sys::Value,
    ret: *mut sys::Value,
) {
    // Create an attrset builder with capacity for 4 attributes.
    let builder = cpp::make_bindings_builder(state, 4);

    // Integer attribute.
    let value = cpp::alloc_value(state);
    sys::init_int(ctx, value, 42);
    let symbol = cpp::create_symbol(state, c"count".as_ptr());
    cpp::bindings_builder_insert(builder, symbol, value);
    cpp::free_symbol(symbol);

    // Boolean attribute.
    let value = cpp::alloc_value(state);
    sys::init_bool(ctx, value, true);
    let symbol = cpp::create_symbol(state, c"enabled".as_ptr());
    cpp::bindings_builder_insert(builder, symbol, value);
    cpp::free_symbol(symbol);

    // Function attribute.
    let mut double_args: [*const c_char; 2] = [c"n".as_ptr(), ptr::null()];
    let double_primop = sys::alloc_primop(
        ctx,
        Some(double),
        1,
        c"double".as_ptr(),
        double_args.as_mut_ptr(),
        c"Double a number".as_ptr(),
        ptr::null_mut(),
    );
    let value = cpp::alloc_value(state);
    sys::init_primop(ctx, value, double_primop);
    let symbol = cpp::create_symbol(state, c"double".as_ptr());
    cpp::bindings_builder_insert(builder, symbol, value);
    sys::gc_decref(ctx, double_primop as *const c_void);
    cpp::free_symbol(symbol);

    // String attribute.
    let value = cpp::alloc_value(state);
    sys::init_string(ctx, value, c"Hello from Rust!".as_ptr());
    let symbol = cpp::create_symbol(state, c"message".as_ptr());
    cpp::bindings_builder_insert(builder, symbol, value);
    cpp::free_symbol(symbol);

    // Finalize into ret (builder is freed inside cpp_make_attrs).
    cpp::make_attrs(ret, builder);
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
#[allow(unsafe_op_in_unsafe_fn)]
pub unsafe extern "C" fn nix_plugin_entry() {
    let ctx = sys::c_context_create();

    let no_args: [*const c_char; 1] = [ptr::null()];

    let primop = sys::alloc_primop(
        ctx,
        Some(jettison_lib),
        0,
        c"jettison".as_ptr(),
        no_args.as_ptr() as *mut _,
        c"nix-jettison's library functions".as_ptr(),
        ptr::null_mut(),
    );
    sys::register_primop(ctx, primop);
    sys::gc_decref(ctx, primop as *const c_void);

    sys::c_context_free(ctx);
}
