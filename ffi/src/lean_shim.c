#include <lean/lean.h>

extern lean_obj_res lean_mk_string_from_bytes(const char * value, size_t size);
extern lean_obj_res lean_mk_io_user_error(lean_obj_arg msg);

LEAN_EXPORT lean_obj_res formal_web_lean_io_result_mk_ok_unit(void) {
    return lean_io_result_mk_ok(lean_box(0));
}

LEAN_EXPORT lean_obj_res formal_web_lean_io_result_mk_error_from_bytes(const char * value, size_t size) {
    lean_obj_res message = lean_mk_string_from_bytes(value, size);
    lean_obj_res error = lean_mk_io_user_error(message);
    return lean_io_result_mk_error(error);
}

LEAN_EXPORT uint8_t formal_web_lean_io_result_is_ok(b_lean_obj_arg result) {
    return lean_io_result_is_ok(result);
}

LEAN_EXPORT void formal_web_lean_io_result_show_error(b_lean_obj_arg result) {
    lean_io_result_show_error(result);
}

LEAN_EXPORT const char * formal_web_lean_string_cstr(b_lean_obj_arg value) {
    return lean_string_cstr(value);
}

LEAN_EXPORT void formal_web_lean_dec(lean_obj_arg value) {
    lean_dec(value);
}