#include "nix/expr/primops.hh"
#include "nix/expr/eval.hh"

using namespace nix;

// Attrsets.

extern "C" nix::BindingsBuilder* make_bindings_builder(EvalState* state, size_t capacity) {
    // buildBindings returns by value, so we allocate on heap.
    auto* builder = new nix::BindingsBuilder(state->buildBindings(capacity));
    return builder;
}

extern "C" void bindings_builder_insert(nix::BindingsBuilder* builder, Symbol* symbol, Value* value) {
    builder->insert(*symbol, value);
}

extern "C" void make_attrs(Value* v, nix::BindingsBuilder* builder) {
    v->mkAttrs(*builder);
    delete builder;
}

// Symbols.

extern "C" Symbol* create_symbol(EvalState* state, const char* name) {
    // Allocate on heap to avoid returning address of temporary.
    Symbol* sym = new Symbol(state->symbols.create(name));
    return sym;
}

extern "C" void free_symbol(Symbol* symbol) {
    delete symbol;
}

// Values.

extern "C" Value* alloc_value(EvalState* state) {
    return state->allocValue();
}
