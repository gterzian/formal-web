#ifndef URL_SESSION_WRAPPER_H
#define URL_SESSION_WRAPPER_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct fw_url_session *fw_url_session_t;

// Completion callback invoked exactly once when a data task finishes.
// All pointer arguments are only valid for the duration of the call;
// the caller must copy what it needs. `error` is NULL on success.
typedef void (*fw_url_session_completion)(
    void *context,
    int status_code,
    const char *final_url,
    const char *content_type,
    const uint8_t *body,
    size_t body_length,
    const char *error);

// Create a session with no shared cache. Returns NULL on failure.
fw_url_session_t fw_url_session_create(void);

// Release a session. NULL-safe.
void fw_url_session_release(fw_url_session_t session);

// Start a data task on the session. `header_names` and `header_values` are
// parallel arrays of `header_count` NUL-terminated strings, appended to the
// request in order; entries are only read for the duration of the call.
// Returns 0 when the task was started (the completion callback will be
// invoked later, on a background queue), non-zero when the task could not be
// started (the completion callback is not invoked).
int fw_url_session_fetch(
    fw_url_session_t session,
    const char *method,
    const char *url,
    const char *const *header_names,
    const char *const *header_values,
    size_t header_count,
    const uint8_t *body,
    size_t body_length,
    void *context,
    fw_url_session_completion completion);

#ifdef __cplusplus
}
#endif

#endif /* URL_SESSION_WRAPPER_H */
