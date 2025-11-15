#include "nix/expr/primops.hh"
#include "nix/expr/eval.hh"

using namespace nix;

extern "C" Value* alloc_value(EvalState* state) {
    return state->allocValue();
}

extern "C" void* create_symbol(EvalState* state, const char* name) {
    // Allocate on heap to avoid returning address of temporary
    Symbol* sym = new Symbol(state->symbols.create(name));
    return (void*)sym;
}

extern "C" void* make_bindings_builder(EvalState* state, size_t capacity) {
    // buildBindings returns by value, so we allocate on heap
    auto* builder = new nix::BindingsBuilder(state->buildBindings(capacity));
    return builder;
}

extern "C" void bindings_builder_insert(void* builder_ptr, void* symbol_ptr, Value* value) {
    auto* builder = static_cast<nix::BindingsBuilder*>(builder_ptr);
    auto* symbol = static_cast<const nix::Symbol*>(symbol_ptr);
    builder->insert(*symbol, value);
}

extern "C" void make_attrs(Value* v, void* builder_ptr) {
    auto* builder = static_cast<nix::BindingsBuilder*>(builder_ptr);
    v->mkAttrs(*builder);
    delete builder;
}

extern "C" void free_symbol(void* symbol_ptr) {
    delete static_cast<Symbol*>(symbol_ptr);
}
