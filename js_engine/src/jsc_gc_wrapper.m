// Objective-C implementation of the JavaScriptCore GC wrapper (see
// `jsc_gc_wrapper.h`).  Compiled with ARC; the opaque handle types are
// Objective-C objects bridged to C with `__bridge_retained`/`__bridge`.

#import <Foundation/Foundation.h>
#import <JavaScriptCore/JavaScriptCore.h>

#include "jsc_gc_wrapper.h"

// An owner object bridged into a JS runtime.  Managed values registered
// against `owner` are retained while this object is alive.
@interface FwJscGcOwner : NSObject
@property (nonatomic, strong) JSContext *context;
@property (nonatomic, strong) id owner;
@property (nonatomic, copy) NSString *key;
@end

@implementation FwJscGcOwner
@end

// A retained `JSManagedValue` plus the context it belongs to.
@interface FwJscManagedValue : NSObject
@property (nonatomic, strong) JSManagedValue *managed;
@property (nonatomic, strong) JSContext *context;
@end

@implementation FwJscManagedValue
@end

// One counter per process is enough; the content process is single-threaded
// and the key only has to be unique among the owners alive at one time.
static uint64_t g_owner_counter = 0;

fw_jsc_gc_owner_t *fw_jsc_gc_owner_create(JSGlobalContextRef global) {
    if (!global) {
        return NULL;
    }
    @autoreleasepool {
        JSContext *context = [JSContext contextWithJSGlobalContextRef:global];
        if (!context) {
            return NULL;
        }
        FwJscGcOwner *handle = [[FwJscGcOwner alloc] init];
        handle.context = context;
        handle.owner = [[NSObject alloc] init];
        handle.key = [NSString
            stringWithFormat:@"__formal_web_gc_owner_%llu", (unsigned long long)++g_owner_counter];
        // Bridging the owner into the JS object graph is what lets
        // JavaScriptCore associate the managed references with it.
        context[handle.key] = handle.owner;
        return (__bridge_retained fw_jsc_gc_owner_t *)handle;
    }
}

void fw_jsc_gc_owner_release(fw_jsc_gc_owner_t *owner) {
    if (!owner) {
        return;
    }
    @autoreleasepool {
        FwJscGcOwner *handle = (__bridge_transfer FwJscGcOwner *)owner;
        // Removing the bridge first drops the JS-side reference so the owner
        // and the managed references it carries can be released.
        handle.context[handle.key] = nil;
        handle.owner = nil;
        handle.context = nil;
    }
}

fw_jsc_managed_value_t *fw_jsc_managed_value_create(
    JSGlobalContextRef global, JSValueRef value, fw_jsc_gc_owner_t *owner)
{
    if (!global || !value) {
        return NULL;
    }
    @autoreleasepool {
        JSContext *context = [JSContext contextWithJSGlobalContextRef:global];
        if (!context) {
            return NULL;
        }
        JSValue *jsValue = [JSValue valueWithJSValueRef:value inContext:context];
        if (!jsValue) {
            return NULL;
        }
        JSManagedValue *managed;
        if (owner) {
            FwJscGcOwner *ownerHandle = (__bridge FwJscGcOwner *)owner;
            managed = [JSManagedValue managedValueWithValue:jsValue andOwner:ownerHandle.owner];
        } else {
            managed = [JSManagedValue managedValueWithValue:jsValue];
        }
        if (!managed) {
            return NULL;
        }
        FwJscManagedValue *handle = [[FwJscManagedValue alloc] init];
        handle.managed = managed;
        handle.context = context;
        return (__bridge_retained fw_jsc_managed_value_t *)handle;
    }
}

fw_jsc_managed_value_t *fw_jsc_managed_value_retain(fw_jsc_managed_value_t *managed) {
    if (!managed) {
        return NULL;
    }
    // Build a distinct wrapper sharing the same `JSManagedValue`, so that
    // releasing one handle (which nils its own fields) cannot disturb the
    // other.
    FwJscManagedValue *handle = (__bridge FwJscManagedValue *)managed;
    FwJscManagedValue *copy = [[FwJscManagedValue alloc] init];
    copy.managed = handle.managed;
    copy.context = handle.context;
    return (__bridge_retained fw_jsc_managed_value_t *)copy;
}

void fw_jsc_managed_value_release(fw_jsc_managed_value_t *managed) {
    if (!managed) {
        return;
    }
    @autoreleasepool {
        FwJscManagedValue *handle = (__bridge_transfer FwJscManagedValue *)managed;
        handle.managed = nil;
        handle.context = nil;
    }
}

JSValueRef fw_jsc_managed_value_get(fw_jsc_managed_value_t *managed) {
    if (!managed) {
        return NULL;
    }
    @autoreleasepool {
        FwJscManagedValue *handle = (__bridge FwJscManagedValue *)managed;
        JSValue *value = handle.managed.value;
        if (!value) {
            return NULL;
        }
        return value.JSValueRef;
    }
}
