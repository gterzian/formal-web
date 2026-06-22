#ifndef XPC_WRAPPER_H
#define XPC_WRAPPER_H

#include <xpc/xpc.h>
#include <dispatch/dispatch.h>

#ifdef __cplusplus
extern "C" {
#endif

// Callback types for XPC events.
typedef void (*xpc_listener_event_callback)(xpc_object_t event, void* context);
typedef void (*xpc_peer_message_callback)(xpc_object_t dictionary, void* context);

// Create a listener connection (server side) with the event handler pre-set.
xpc_connection_t fw_xpc_create_listener(
    const char* service_name,
    dispatch_queue_t queue,
    xpc_listener_event_callback callback,
    void* context
);

// Create a client connection to a named service with the message handler pre-set.
xpc_connection_t fw_xpc_create_client(
    const char* service_name,
    dispatch_queue_t queue,
    xpc_peer_message_callback callback,
    void* context
);

// Set the event handler on an existing listener connection.
void fw_xpc_set_listener_handler(
    xpc_connection_t listener,
    dispatch_queue_t queue,
    xpc_listener_event_callback callback,
    void* context
);

// Set the message handler on an existing peer/client connection.
void fw_xpc_set_peer_handler(
    xpc_connection_t peer,
    dispatch_queue_t queue,
    xpc_peer_message_callback callback,
    void* context
);

// Create a connection to an embedded XPC service (bypasses launchd).
// Services inside XPCServices/ are found in the app bundle.
xpc_connection_t fw_xpc_create_connection(
    const char* service_name,
    dispatch_queue_t queue,
    xpc_peer_message_callback callback,
    void* context
);

// Extract a peer connection from a listener event.
xpc_connection_t fw_xpc_peer_from_event(xpc_object_t event);

// Resume a connection (required to start receiving messages).
void fw_xpc_resume(xpc_connection_t connection);

// Cancel a connection.
void fw_xpc_cancel(xpc_connection_t connection);

// Run the XPC service event loop (calls xpc_main internally).
// Never returns.
void fw_xpc_run_service(void (*handler)(xpc_connection_t, void*), void* context);

#ifdef __cplusplus
}
#endif

#endif /* XPC_WRAPPER_H */
