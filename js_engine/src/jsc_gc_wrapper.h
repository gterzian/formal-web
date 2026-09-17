#ifndef FW_JSC_GC_WRAPPER_H
#define FW_JSC_GC_WRAPPER_H

// Objective-C wrapper around JavaScriptCore's `JSManagedValue` and the
// `JSVirtualMachine` managed-reference API.
//
// `JSManagedValue` is the public API for holding a JS value from the
// Objective-C side.  It replaces the undocumented `JSValueProtect` /
// `JSValueUnprotect` C functions: the value is retained while the managed
// value object is alive and released with it, so no separate protect set has
// to be balanced by hand.
//
// The managed value is *weak* when created without an owner (`managed_value`
// returns NULL once the JS value is collected) and *conditionally retained*
// when created with an owner: the JS value is kept alive while the owner
// object is alive, and registering the owner with the virtual machine lets
// JavaScriptCore break cross-language reference cycles.

#include <JavaScriptCore/JavaScriptCore.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct fw_jsc_gc_owner fw_jsc_gc_owner_t;
typedef struct fw_jsc_managed_value fw_jsc_managed_value_t;

// Create an owner object for `global`, bridged into the JS runtime so
// JavaScriptCore tracks managed references registered against it.  The owner
// must be released with `fw_jsc_gc_owner_release`; managed values registered
// with it are only retained while it is alive.  Returns NULL on failure.
fw_jsc_gc_owner_t *fw_jsc_gc_owner_create(JSGlobalContextRef global);

// Release an owner created by `fw_jsc_gc_owner_create`.  NULL-safe.
void fw_jsc_gc_owner_release(fw_jsc_gc_owner_t *owner);

// Create a managed value wrapping `value`.
//
// With a non-NULL `owner` the value is conditionally retained: it stays alive
// while `owner` is alive.  With a NULL `owner` it is a weak reference: it
// survives only while the value is reachable through the JS object graph.
// The returned handle is retained; release it with
// `fw_jsc_managed_value_release`.  Returns NULL on failure.
fw_jsc_managed_value_t *fw_jsc_managed_value_create(
    JSGlobalContextRef global,
    JSValueRef value,
    fw_jsc_gc_owner_t *owner);

// Retain `managed` and return the same handle (for cloning a Rust-side
// reference).  NULL-safe.
fw_jsc_managed_value_t *fw_jsc_managed_value_retain(fw_jsc_managed_value_t *managed);

// Release a handle from `fw_jsc_managed_value_create`/`_retain`.  NULL-safe.
void fw_jsc_managed_value_release(fw_jsc_managed_value_t *managed);

// The current value, or NULL if it has been collected.  A value returned for
// a weak managed value is only guaranteed alive until the next JavaScript
// execution.
JSValueRef fw_jsc_managed_value_get(fw_jsc_managed_value_t *managed);

#ifdef __cplusplus
}
#endif

#endif /* FW_JSC_GC_WRAPPER_H */
