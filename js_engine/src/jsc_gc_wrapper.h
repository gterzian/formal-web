#ifndef FW_JSC_GC_WRAPPER_H
#define FW_JSC_GC_WRAPPER_H

// Objective-C wrapper around JavaScriptCore's ObjC API.
//
// Two jobs:
//
// 1. **Export platform objects to JS.**  A `JSManagedValue` only retains its
//    referent when its owner is an ObjC object that JavaScriptCore has bridged
//    into the JS graph.  A C-API `JSObjectMake` object has no ObjC identity, so
//    each platform object is represented by an exported ObjC holder
//    (`fw_jsc_platform_object_*`).  The holder owns the Rust platform data and
//    is the owner for every managed value the platform object holds.  The JS
//    object itself is the holder's bridge-created wrapper, so it can still be
//    shaped with the C API (prototype, members, descriptors).
//
// 2. **Managed references.**  `JSManagedValue` replaces the hand-balanced
//    `JSValueProtect`/`JSValueUnprotect` protect set.  A managed value created
//    with a platform-object (or realm) owner is retained while that owner is
//    reachable from JS, so it lives exactly as long as the object that owns it
//    and cross-language cycles are collectable.

#include <JavaScriptCore/JavaScriptCore.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct fw_jsc_context fw_jsc_context_t;
typedef struct fw_jsc_gc_owner fw_jsc_gc_owner_t;
typedef struct fw_jsc_platform_object fw_jsc_platform_object_t;
typedef struct fw_jsc_managed_value fw_jsc_managed_value_t;

// ── Context ───────────────────────────────────────────────────────────────

// A cached Objective-C `JSContext` wrapper for `global`.  Held by the Rust
// engine for the engine's lifetime so the ObjC↔JS association is stable.
fw_jsc_context_t *fw_jsc_context_create(JSGlobalContextRef global);
void fw_jsc_context_release(fw_jsc_context_t *context);

// ── Realm owner ───────────────────────────────────────────────────────────

// A realm-lifetime owner, bridged into the JS graph as a property of the
// context's global object.  Used for the handful of references the realm
// itself roots (the Window and Document).  NULL-safe release.
fw_jsc_gc_owner_t *fw_jsc_gc_owner_create(fw_jsc_context_t *context);
void fw_jsc_gc_owner_release(fw_jsc_gc_owner_t *owner);

// ── Platform objects ──────────────────────────────────────────────────────

// Create an exported platform-object holder owning `data`.  The holder frees
// `data` by calling `fw_jsc_platform_object_drop_data(data, jsObject)` when the
// JS object is collected.  Returns a retained handle; release it with
// `fw_jsc_platform_object_release` after reading the JS object.
fw_jsc_platform_object_t *fw_jsc_platform_object_create(
    fw_jsc_context_t *context, void *data);

// The JS object for an exported holder.  Valid while the holder (or its JS
// wrapper) is alive.
JSObjectRef fw_jsc_platform_object_js_object(
    fw_jsc_context_t *context, fw_jsc_platform_object_t *holder);

// Release a handle returned by `fw_jsc_platform_object_create`.  The holder
// stays alive as long as its JS wrapper is reachable.
void fw_jsc_platform_object_release(fw_jsc_platform_object_t *holder);

// ── Managed values ────────────────────────────────────────────────────────

// Wrap `value`.  `owner` is a platform-object holder or a realm owner, or NULL
// for a weak reference (retained only while the JS value is reachable from the
// JS graph).  The returned handle is retained; release it with
// `fw_jsc_managed_value_release`.
fw_jsc_managed_value_t *fw_jsc_managed_value_create(
    fw_jsc_context_t *context, JSValueRef value, void *owner);

fw_jsc_managed_value_t *fw_jsc_managed_value_retain(fw_jsc_managed_value_t *managed);
void fw_jsc_managed_value_release(fw_jsc_managed_value_t *managed);

// The current value, or NULL if it has been collected.
JSValueRef fw_jsc_managed_value_get(fw_jsc_managed_value_t *managed);

#ifdef __cplusplus
}
#endif

#endif /* FW_JSC_GC_WRAPPER_H */
