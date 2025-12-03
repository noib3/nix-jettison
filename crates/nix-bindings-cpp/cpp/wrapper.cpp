#include "nix/expr/primops.hh"
#include "nix/expr/eval.hh"
#include "nix_api_util_internal.h"
#include "nix_api_expr.h"

// Attrsets.

extern "C" nix::BindingsBuilder* make_bindings_builder(nix::EvalState* state, size_t capacity) {
    // buildBindings returns by value, so we allocate on heap.
    auto* builder = new nix::BindingsBuilder(state->buildBindings(capacity));
    return builder;
}

extern "C" void bindings_builder_insert(nix::BindingsBuilder* builder, const char* name, nix::Value* value) {
    nix::Symbol sym = builder->symbols.get().create(name);
    builder->insert(sym, value);
}

extern "C" void make_attrs(nix::Value* v, nix::BindingsBuilder* builder) {
    v->mkAttrs(*builder);
    delete builder;
}

extern "C" nix::Value* get_attr_byname_lazy(const nix::Value* value, nix::EvalState* state, const char* name) {
    nix::Symbol sym = state->symbols.create(name);
    const nix::Attr* attr = value->attrs()->get(sym);
    if (!attr) {
        return nullptr;
    }
    return attr->value;
}

// Attrset iterator.

struct AttrIterator {
    nix::Bindings::const_iterator current;
    const nix::SymbolTable* symbols;
};

extern "C" AttrIterator* attr_iter_create(
    const nix::Value* value,
    nix::EvalState* state
) {
    const nix::Bindings* bindings = value->attrs();
    return new AttrIterator{
        bindings->begin(),
        &state->symbols
    };
}

extern "C" const char* attr_iter_key(const AttrIterator* iter) {
    return (*iter->symbols)[iter->current->name].c_str();
}

extern "C" nix::Value* attr_iter_value(const AttrIterator* iter) {
    return iter->current->value;
}

extern "C" void attr_iter_advance(AttrIterator* iter) {
    ++iter->current;
}

extern "C" void attr_iter_destroy(AttrIterator* iter) {
    delete iter;
}

// Builtins.

extern "C" nix::Value* get_builtins(nix::EvalState* state) {
    // builtins is the first value in baseEnv
    return state->baseEnv.values[0];
}

// Lists.

extern "C" nix::ListBuilder* make_list_builder(nix::EvalState* state, size_t size) {
    auto* builder = new nix::ListBuilder(state->buildList(size));
    return builder;
}

extern "C" void list_builder_insert(nix::ListBuilder* builder, size_t index, nix::Value* value) {
    (*builder)[index] = value;
}

extern "C" void make_list(nix::Value* v, nix::ListBuilder* builder) {
    v->mkList(*builder);
    delete builder;
}

// Values.

extern "C" nix::Value* alloc_value(nix::EvalState* state) {
    nix::Value* res = state->allocValue();
    nix_gc_incref(nullptr, res);
    return res;
}

extern "C" void force_value(nix::EvalState* state, nix::Value* value) {
    state->forceValue(*value, nix::noPos);
}

extern "C" void init_path_string(nix::EvalState* state, nix::Value* value, const char* str) {
    value->mkPath(state->rootPath(nix::CanonPath(str)));
}

extern "C" nix_err value_call_multi(
    nix_c_context* context,
    nix::EvalState* state,
    nix::Value* fn,
    size_t nargs,
    nix::Value** args,
    nix::Value* result
) {
    if (context)
        context->last_err_code = NIX_OK;
    try {
        state->callFunction(*fn, {args, nargs}, *result, nix::noPos);
        state->forceValue(*result, nix::noPos);
    }
    NIXC_CATCH_ERRS
}
