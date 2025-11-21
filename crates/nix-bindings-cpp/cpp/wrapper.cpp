#include "nix/expr/primops.hh"
#include "nix/expr/eval.hh"

using namespace nix;

// Attrsets.

extern "C" nix::BindingsBuilder* make_bindings_builder(EvalState* state, size_t capacity) {
    // buildBindings returns by value, so we allocate on heap.
    auto* builder = new nix::BindingsBuilder(state->buildBindings(capacity));
    return builder;
}

extern "C" void bindings_builder_insert(nix::BindingsBuilder* builder, const char* name, Value* value) {
    Symbol sym = builder->symbols.get().create(name);
    builder->insert(sym, value);
}

extern "C" void make_attrs(Value* v, nix::BindingsBuilder* builder) {
    v->mkAttrs(*builder);
    delete builder;
}

// Values.

extern "C" Value* alloc_value(EvalState* state) {
    return state->allocValue();
}

extern "C" void force_value(EvalState* state, Value* value) {
    state->forceValue(*value, nix::noPos);
}
