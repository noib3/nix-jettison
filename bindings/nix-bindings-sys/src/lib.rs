#![allow(missing_docs)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

/// The maximum arity of a primitive operation.
///
/// See [this][source] for more infos.
///
/// [source]: https://github.com/NixOS/nix/blob/2.32.2/src/libexpr/include/nix/expr/eval.hh#L33-L38
pub const MAX_PRIMOP_ARITY: u8 = 8;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
