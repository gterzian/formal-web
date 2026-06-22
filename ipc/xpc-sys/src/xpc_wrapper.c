#include "xpc_wrapper.h"
#include <dispatch/dispatch.h>
#include <xpc/xpc.h>

// ── Listener connection creation ────────────────────────────────────────────
//
// All events (including XPC_TYPE_ERROR) are forwarded to the callback.
// The block captures callback and context by value — no malloc needed.

xpc_connection_t fw_xpc_create_listener(
    const char* service_name,
    dispatch_queue_t queue,
    xpc_listener_event_callback callback,
    void* context)
{
    xpc_connection_t listener = xpc_connection_create_mach_service(
        service_name,
        queue,
        XPC_CONNECTION_MACH_SERVICE_LISTENER
    );

    xpc_connection_set_event_handler(listener, ^(xpc_object_t event) {
        if (callback) {
            if ((uintptr_t)event > 0x100000) {
                callback(event, context);
            } else {
                callback(NULL, context);
            }
        }
    });

    return listener;
}

// ── Client connection creation ──────────────────────────────────────────────
//
// Forward ALL event types (XPC_TYPE_DICTIONARY and XPC_TYPE_ERROR) to Rust.
// The Rust layer parses XPCErrorDescription from error objects to detect
// invalidation and close crossbeam channels. Swallowing errors here causes
// the parent process to deadlock when a helper exits unexpectedly.

xpc_connection_t fw_xpc_create_client(
    const char* service_name,
    dispatch_queue_t queue,
    xpc_peer_message_callback callback,
    void* context)
{
    xpc_connection_t conn = xpc_connection_create_mach_service(
        service_name,
        queue,
        0 // Not a listener
    );

    xpc_connection_set_event_handler(conn, ^(xpc_object_t object) {
        if (callback) {
            if ((uintptr_t)object > 0x100000) {
                callback(object, context);
            } else {
                callback(NULL, context);
            }
        }
    });

    return conn;
}

// ── Setting handlers on existing connections ────────────────────────────────

void fw_xpc_set_listener_handler(
    xpc_connection_t listener,
    dispatch_queue_t queue,
    xpc_listener_event_callback callback,
    void* context)
{
    xpc_connection_set_target_queue(listener, queue);
    xpc_connection_set_event_handler(listener, ^(xpc_object_t event) {
        if (callback) {
            // On macOS 26+, invalid objects can be delivered as small integers.
            // Pass NULL for non-pointer values so Rust handles them safely.
            if ((uintptr_t)event > 0x100000) {
                callback(event, context);
            } else {
                callback(NULL, context);
            }
        }
    });
}

void fw_xpc_set_peer_handler(
    xpc_connection_t peer,
    dispatch_queue_t queue,
    xpc_peer_message_callback callback,
    void* context)
{
    xpc_connection_set_target_queue(peer, queue);
    xpc_connection_set_event_handler(peer, ^(xpc_object_t object) {
        if (callback) {
            // On macOS 26+, mach cancel events deliver small integer values
            // instead of XPC objects. Pass NULL so Rust handles invalidation.
            if ((uintptr_t)object > 0x100000) {
                callback(object, context);
            } else {
                callback(NULL, context);
            }
        }
    });
}

// ── Embedded XPC service connection (bypasses launchd) ────────────────────
//
// For embedded XPC services inside XPCServices/, use xpc_connection_create.
// This finds the service in the app bundle and launches it directly, bypassing
// launchd's Mach bootstrap lookup (and its watchdog for large binaries).

xpc_connection_t fw_xpc_create_connection(
    const char* service_name,
    dispatch_queue_t queue,
    xpc_peer_message_callback callback,
    void* context)
{
    xpc_connection_t conn = xpc_connection_create(service_name, queue);

    xpc_connection_set_event_handler(conn, ^(xpc_object_t object) {
        if (callback) {
            if ((uintptr_t)object > 0x100000) {
                callback(object, context);
            } else {
                callback(NULL, context);
            }
        }
    });

    return conn;
}

// ── Utility functions ───────────────────────────────────────────────────────

xpc_connection_t fw_xpc_peer_from_event(xpc_object_t event)
{
    return (xpc_connection_t)event;
}

void fw_xpc_resume(xpc_connection_t connection)
{
    xpc_connection_resume(connection);
}

void fw_xpc_cancel(xpc_connection_t connection)
{
    xpc_connection_cancel(connection);
}

// ── XPC service main loop ──────────────────────────────────────────────────

// Thread-local storage for the service handler callback, since xpc_main
// takes a plain function pointer (not a block) with no context parameter.
static __thread void (*fw_xpc_service_handler)(xpc_connection_t, void*) = NULL;
static __thread void* fw_xpc_service_context = NULL;

static void fw_xpc_service_trampoline(xpc_connection_t conn)
{
    if (fw_xpc_service_handler) {
        fw_xpc_service_handler(conn, fw_xpc_service_context);
    }
}

void fw_xpc_run_service(void (*handler)(xpc_connection_t, void*), void* context)
{
    fw_xpc_service_handler = handler;
    fw_xpc_service_context = context;
    xpc_main(fw_xpc_service_trampoline);
}
