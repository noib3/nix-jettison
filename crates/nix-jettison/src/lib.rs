use core::ffi::{c_char, c_void};
use core::ptr;

use nix_bindings_sys::{
    EvalState,
    nix_alloc_primop,
    nix_c_context,
    nix_c_context_create,
    nix_c_context_free,
    nix_gc_decref,
    nix_init_int,
    nix_register_primop,
    nix_value,
};

const NO_ARGS: &[*const c_char; 1] = &[ptr::null_mut()];

#[allow(unsafe_op_in_unsafe_fn)]
unsafe extern "C" fn jettison_lib(
    _user_data: *mut c_void,
    ctx: *mut nix_c_context,
    _state: *mut EvalState,
    _args: *mut *mut nix_value,
    ret: *mut nix_value,
) {
    nix_init_int(ctx, ret, 0);
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
