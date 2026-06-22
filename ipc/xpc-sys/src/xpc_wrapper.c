#include "xpc_wrapper.h"
#include <dispatch/dispatch.h>
#include <xpc/xpc.h>
#include <stdlib.h>
#include <string.h>

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
            callback(event, context);
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
            callback(object, context);
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
            callback(event, context);
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
            callback(object, context);
        }
    });
}

// ── Utility functions ───────────────────────────────────────────────────────

xpc_connection_t fw_xpc_peer_from_event(xpc_object_t event)
{
    // For listener connections, the event IS the peer connection.
    // XPC delivers connections as xpc_connection_t objects through
    // the event handler. The retain count is already managed by XPC.
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
