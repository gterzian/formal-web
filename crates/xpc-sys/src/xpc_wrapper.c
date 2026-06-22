#include "xpc_wrapper.h"
#include <dispatch/dispatch.h>
#include <xpc/xpc.h>
#include <stdlib.h>
#include <string.h>

// Context structs for callback storage.
struct listener_ctx {
    xpc_listener_event_callback callback;
    void* context;
};

struct peer_ctx {
    xpc_peer_message_callback callback;
    void* context;
};

// ── Listener connection creation ────────────────────────────────────────────

xpc_connection_t fw_xpc_create_listener(
    const char* service_name,
    dispatch_queue_t queue,
    xpc_listener_event_callback callback,
    void* context)
{
    struct listener_ctx* ctx = malloc(sizeof(struct listener_ctx));
    ctx->callback = callback;
    ctx->context = context;

    xpc_connection_t listener = xpc_connection_create_mach_service(
        service_name,
        queue,
        XPC_CONNECTION_MACH_SERVICE_LISTENER
    );

    xpc_connection_set_event_handler(listener, ^(xpc_object_t event) {
        ctx->callback(event, ctx->context);
    });

    return listener;
}

// ── Client connection creation ──────────────────────────────────────────────

xpc_connection_t fw_xpc_create_client(
    const char* service_name,
    dispatch_queue_t queue,
    xpc_peer_message_callback callback,
    void* context)
{
    struct peer_ctx* ctx = malloc(sizeof(struct peer_ctx));
    ctx->callback = callback;
    ctx->context = context;

    xpc_connection_t conn = xpc_connection_create_mach_service(
        service_name,
        queue,
        0 // Not a listener
    );

    xpc_connection_set_event_handler(conn, ^(xpc_object_t object) {
        xpc_type_t type = xpc_get_type(object);
        if (type == XPC_TYPE_DICTIONARY) {
            ctx->callback(object, ctx->context);
        } else if (type == XPC_TYPE_ERROR) {
            const char* desc = xpc_dictionary_get_string(object, "XPCErrorDescription");
            if (desc) {
                // Error events are handled by checking the error description.
                // Don't forward to the message callback.
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
    struct listener_ctx* ctx = malloc(sizeof(struct listener_ctx));
    ctx->callback = callback;
    ctx->context = context;

    xpc_connection_set_target_queue(listener, queue);
    xpc_connection_set_event_handler(listener, ^(xpc_object_t event) {
        ctx->callback(event, ctx->context);
    });
}

void fw_xpc_set_peer_handler(
    xpc_connection_t peer,
    dispatch_queue_t queue,
    xpc_peer_message_callback callback,
    void* context)
{
    struct peer_ctx* ctx = malloc(sizeof(struct peer_ctx));
    ctx->callback = callback;
    ctx->context = context;

    xpc_connection_set_target_queue(peer, queue);
    xpc_connection_set_event_handler(peer, ^(xpc_object_t object) {
        xpc_type_t type = xpc_get_type(object);
        if (type == XPC_TYPE_DICTIONARY) {
            ctx->callback(object, ctx->context);
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
