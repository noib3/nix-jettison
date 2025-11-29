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

extern "C" Value* get_attr_byname_lazy(const Value* value, EvalState* state, const char* name) {
    Symbol sym = state->symbols.create(name);
    const Attr* attr = value->attrs()->get(sym);
    if (!attr) {
        return nullptr;
    }
    return attr->value;
}

// Builtins.

extern "C" Value* get_builtins(EvalState* state) {
    // builtins is the first value in baseEnv
    return state->baseEnv.values[0];
}

// Lists.

extern "C" nix::ListBuilder* make_list_builder(EvalState* state, size_t size) {
    auto* builder = new nix::ListBuilder(state->buildList(size));
    return builder;
}

extern "C" void list_builder_insert(nix::ListBuilder* builder, size_t index, Value* value) {
    (*builder)[index] = value;
}

extern "C" void make_list(Value* v, nix::ListBuilder* builder) {
    v->mkList(*builder);
    delete builder;
}

// Values.

extern "C" Value* alloc_value(EvalState* state) {
    return state->allocValue();
}

extern "C" void force_value(EvalState* state, Value* value) {
    state->forceValue(*value, nix::noPos);
}

extern "C" void init_path_string(EvalState* state, Value* value, const char* str) {
    value->mkPath(state->rootPath(nix::CanonPath(str)));
}
