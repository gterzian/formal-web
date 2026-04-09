#include <stdbool.h>
#include <lean/lean.h>

extern void lean_initialize(void);
extern lean_obj_res lean_mk_string_from_bytes(const char * value, size_t size);
extern lean_obj_res lean_mk_io_user_error(lean_obj_arg msg);
extern lean_obj_res initialize_formal_x2dweb_FormalWebRuntime(uint8_t builtin);

static bool formal_web_lean_runtime_initialized = false;

LEAN_EXPORT lean_obj_res formalWebInitializeLeanRuntime(void) {
    if (formal_web_lean_runtime_initialized) {
        return lean_io_result_mk_ok(lean_box(0));
    }

    lean_initialize();
    lean_set_panic_messages(false);
    lean_obj_res result = initialize_formal_x2dweb_FormalWebRuntime(1);
    lean_set_panic_messages(true);
    if (lean_io_result_is_error(result)) {
        return result;
    }

    lean_dec_ref(result);
    lean_io_mark_end_initialization();
    lean_init_task_manager();
    formal_web_lean_runtime_initialized = true;
    return lean_io_result_mk_ok(lean_box(0));
}

LEAN_EXPORT lean_obj_res formalWebFinalizeLeanRuntime(void) {
    if (formal_web_lean_runtime_initialized) {
        lean_finalize_task_manager();
        formal_web_lean_runtime_initialized = false;
    }

    return lean_io_result_mk_ok(lean_box(0));
}

LEAN_EXPORT lean_obj_res leanIoResultMkOkUnit(void) {
    return lean_io_result_mk_ok(lean_box(0));
}

LEAN_EXPORT lean_obj_res leanIoResultMkOkUsize(size_t value) {
    return lean_io_result_mk_ok(lean_box_usize(value));
}

LEAN_EXPORT lean_obj_res leanIoResultMkErrorFromBytes(const char * value, size_t size) {
    lean_obj_res message = lean_mk_string_from_bytes(value, size);
    lean_obj_res error = lean_mk_io_user_error(message);
    return lean_io_result_mk_error(error);
}

LEAN_EXPORT uint8_t leanIoResultIsOk(b_lean_obj_arg result) {
    return lean_io_result_is_ok(result);
}

LEAN_EXPORT void leanIoResultShowError(b_lean_obj_arg result) {
    lean_io_result_show_error(result);
}

LEAN_EXPORT const char * leanStringCstr(b_lean_obj_arg value) {
    return lean_string_cstr(value);
}

LEAN_EXPORT size_t leanByteArraySize(b_lean_obj_arg value) {
    return lean_sarray_size(value);
}

LEAN_EXPORT const uint8_t * leanByteArrayCptr(b_lean_obj_arg value) {
    return lean_sarray_cptr(value);
}

LEAN_EXPORT size_t leanUnboxUsize(b_lean_obj_arg value) {
    return lean_unbox_usize(value);
}

LEAN_EXPORT void leanDec(lean_obj_arg value) {
    lean_dec(value);
}