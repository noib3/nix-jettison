use core::ffi::{c_char, c_void};
use core::ptr;

use nix_bindings_sys::{
    EvalState,
    Value,
    nix_alloc_primop,
    nix_alloc_value,
    nix_bindings_builder_free,
    nix_bindings_builder_insert,
    nix_c_context,
    nix_c_context_create,
    nix_c_context_free,
    nix_gc_decref,
    nix_get_int,
    nix_init_int,
    nix_init_primop,
    nix_make_attrs,
    nix_make_bindings_builder,
    nix_register_primop,
    nix_value_decref,
};

const NO_ARGS: &[*const c_char; 1] = &[ptr::null_mut()];

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "C" fn add(
    _user_data: *mut c_void,
    ctx: *mut nix_c_context,
    _state: *mut EvalState,
    args: *mut *mut Value,
    ret: *mut Value,
) {
    let a = nix_get_int(ctx, *args.offset(0));
    let b = nix_get_int(ctx, *args.offset(1));
    nix_init_int(ctx, ret, a + b);
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "C" fn jettison_lib(
    _user_data: *mut c_void,
    ctx: *mut nix_c_context,
    state: *mut EvalState,
    _args: *mut *mut Value,
    ret: *mut Value,
) {
    // Create an attrset builder with a capacity of 1.
    let builder = nix_make_bindings_builder(ctx, state, 1);

    let add_name = c"add".as_ptr();

    let mut add_args: [*const c_char; 3] =
        [c"a".as_ptr(), c"b".as_ptr(), ptr::null()];

    let add_primop = nix_alloc_primop(
        ctx,
        Some(add),
        2,
        add_name,
        add_args.as_mut_ptr(),
        c"Add two integers together".as_ptr(),
        ptr::null_mut(),
    );

    // Convert the primop into a Value and add it to the builder.
    let add_value = nix_alloc_value(ctx, state);
    nix_init_primop(ctx, add_value, add_primop);
    nix_bindings_builder_insert(ctx, builder, add_name, add_value);

    // Finalize the builder.
    nix_make_attrs(ctx, ret, builder);

    // Clean up.
    nix_bindings_builder_free(builder);
    nix_value_decref(ctx, add_value);
    nix_gc_decref(ctx, add_primop as *const c_void);
}

#[unsafe(no_mangle)]
#[allow(clippy::missing_safety_doc)]
#[allow(unsafe_op_in_unsafe_fn)]
pub unsafe extern "C" fn nix_plugin_entry() {
    let ctx = nix_c_context_create();

    let primop = nix_alloc_primop(
        ctx,
        Some(jettison_lib),
        0,
        c"nix-jettison".as_ptr(),
        NO_ARGS as *const _ as *mut _,
        c"nix-jettison library functions".as_ptr(),
        ptr::null_mut(),
    );

    nix_register_primop(ctx, primop);

    nix_gc_decref(ctx, primop as *const c_void);

    nix_c_context_free(ctx);
}
