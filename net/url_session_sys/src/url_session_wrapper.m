// Minimal Foundation URLSession wrapper for the net crate's URLSession
// backend.
//
// Compiled without ARC (clang's default for .m): object references are
// retained/released manually.

#import <Foundation/Foundation.h>

#include "url_session_wrapper.h"
#include <stdlib.h>
#include <string.h>

struct fw_url_session {
    NSURLSession *session;
};

fw_url_session_t fw_url_session_create(void) {
    NSURLSessionConfiguration *configuration =
        [NSURLSessionConfiguration ephemeralSessionConfiguration];
    // Deliberate: disable the URLCache entirely. `ephemeralSessionConfiguration`
    // alone only avoids persistent storage — it still keeps a non-persistent
    // (in-memory) cache, which would let the session serve cached responses
    // for repeated URLs. The net backend contracts one session per network
    // partition key (event loop) with no shared cache, and does not model
    // HTTP caching (net/README.md: "Will host HTTP cache logic when the Fetch
    // spec reaches that layer"), so every fetch must reach the transport.
    // Re-enabling the URLCache here would change fetch semantics (cached
    // responses with no cache-control handling); do so deliberately, with
    // tests.
    configuration.URLCache = nil;

    NSURLSession *session = [NSURLSession sessionWithConfiguration:configuration];
    fw_url_session_t handle = (fw_url_session_t)calloc(1, sizeof(struct fw_url_session));
    if (handle) {
        handle->session = [session retain];
    }
    return handle;
}

void fw_url_session_release(fw_url_session_t handle) {
    if (!handle) {
        return;
    }
    [handle->session release];
    handle->session = nil;
    free(handle);
}

int fw_url_session_fetch(
    fw_url_session_t handle,
    const char *method,
    const char *url,
    const char *const *header_names,
    const char *const *header_values,
    size_t header_count,
    const uint8_t *body,
    size_t body_length,
    void *context,
    fw_url_session_completion completion)
{
    if (!handle || !handle->session || !url) {
        return -1;
    }

    NSString *urlString = [NSString stringWithUTF8String:url];
    NSURL *nsurl = [NSURL URLWithString:urlString];
    if (!nsurl) {
        return -1;
    }

    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:nsurl];
    request.HTTPMethod = method ? [NSString stringWithUTF8String:method] : @"GET";
    for (size_t index = 0; index < header_count; index++) {
        const char *name = header_names ? header_names[index] : NULL;
        const char *value = header_values ? header_values[index] : NULL;
        if (!name || !value) {
            continue;
        }
        NSString *header_name = [NSString stringWithUTF8String:name];
        NSString *header_value = [NSString stringWithUTF8String:value];
        if (!header_name || !header_value) {
            // Not UTF-8; the header is dropped rather than failing the fetch.
            continue;
        }
        // <https://fetch.spec.whatwg.org/#concept-header-list-combine>
        // Deliberate: `addValue:` combines repeated names with ", ", the way
        // a header list is combined on the wire; `setValue:` would keep only
        // the last value of a repeated name. NSURLSession reserves a few
        // fields (Content-Length, Host, Connection, the authorization pair)
        // and overwrites whatever is set for them here.
        [request addValue:header_value forHTTPHeaderField:header_name];
    }
    if (body && body_length > 0) {
        request.HTTPBody = [NSData dataWithBytes:body length:body_length];
    }

    NSURLSessionDataTask *task = [handle->session
        dataTaskWithRequest:request
        completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
            if (error) {
                const char *message = [[error localizedDescription] UTF8String];
                char *message_copy = message ? strdup(message) : NULL;
                if (completion) {
                    // The callback must never see a NULL error on the error
                    // path: the Rust trampoline treats NULL as success. A
                    // static literal stays valid for the duration of the
                    // call when strdup failed.
                    completion(context, 0, NULL, NULL, NULL, 0,
                               message_copy ? message_copy : "URLSession fetch failed");
                }
                free(message_copy);
                return;
            }

            long status = 0;
            NSString *content_type = nil;
            if ([response isKindOfClass:[NSHTTPURLResponse class]]) {
                NSHTTPURLResponse *http_response = (NSHTTPURLResponse *)response;
                status = [http_response statusCode];
                content_type =
                    [[http_response allHeaderFields] objectForKey:@"Content-Type"];
            }

            NSString *final_url_string = [[response URL] absoluteString];
            const char *final_url_cstring = final_url_string ? [final_url_string UTF8String] : NULL;
            const char *content_type_cstring = content_type ? [content_type UTF8String] : NULL;
            char *final_url_copy = final_url_cstring ? strdup(final_url_cstring) : NULL;
            char *content_type_copy = content_type_cstring ? strdup(content_type_cstring) : NULL;
            const uint8_t *bytes = data ? (const uint8_t *)[data bytes] : NULL;
            size_t length = data ? (size_t)[data length] : 0;

            if (completion) {
                completion(context, (int)status, final_url_copy, content_type_copy, bytes,
                           length, NULL);
            }
            free(final_url_copy);
            free(content_type_copy);
        }];
    [task resume];
    return 0;
}
