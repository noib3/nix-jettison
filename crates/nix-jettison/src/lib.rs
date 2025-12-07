#![allow(missing_docs)]

mod build_crate;
mod build_package;
mod cargo_lock_parser;
mod jettison;
mod resolve_build_graph;
mod vendor_deps;

use nix_bindings::context::{Context, Entrypoint};

#[nix_bindings::entry]
fn jettison(ctx: &mut Context<Entrypoint>) {
    ctx.register_primop::<jettison::Jettison>()
}
