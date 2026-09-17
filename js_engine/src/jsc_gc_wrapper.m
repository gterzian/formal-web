// Objective-C implementation of the JavaScriptCore GC wrapper (see
// `jsc_gc_wrapper.h`).  Compiled with ARC; opaque handles are Objective-C
// objects bridged to C with `__bridge_retained`/`__bridge`.

#import <Foundation/Foundation.h>
#import <JavaScriptCore/JavaScriptCore.h>

#include "jsc_gc_wrapper.h"

// Called by a platform-object holder's `dealloc` to free its Rust data.  The
// JS object is passed so the Rust-side lookup table can drop its entry.
extern void fw_jsc_platform_object_drop_data(void *data, void *jsObject);

// A cached `JSContext` wrapper, held by the Rust engine.
@interface FwJscContext : NSObject
@property (nonatomic, strong) JSContext *context;
@end

@implementation FwJscContext
@end

// A realm-lifetime owner, bridged into the JS graph as a property of the
// context's global object.
@interface FwJscGcOwner : NSObject
@property (nonatomic, strong) JSContext *context;
@property (nonatomic, strong) id owner;
@property (nonatomic, copy) NSString *key;
- (id)managedOwner;
@end

@implementation FwJscGcOwner
- (id)managedOwner {
    return self.owner;
}
@end

// The exported ObjC object standing in for a platform object.  JavaScriptCore
// creates its JS wrapper via the ObjC bridge; that wrapper is the platform
// object's JS object.  The holder owns the Rust platform data and is the
// managed-reference owner for every JS value the platform object holds.
@interface FwJscPlatformObject : NSObject
@property (nonatomic, assign) void *data;
@property (nonatomic, assign) JSValueRef jsObject;
@property (nonatomic, strong) JSContext *context;
- (id)managedOwner;
@end

@implementation FwJscPlatformObject
- (id)managedOwner {
    return self;
}

- (void)dealloc {
    if (self.data) {
        fw_jsc_platform_object_drop_data(self.data, (void *)self.jsObject);
        self.data = NULL;
    }
}
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

fw_jsc_context_t *fw_jsc_context_create(JSGlobalContextRef global) {
    if (!global) {
        return NULL;
    }
    @autoreleasepool {
        JSContext *context = [JSContext contextWithJSGlobalContextRef:global];
        if (!context) {
            return NULL;
        }
        FwJscContext *handle = [[FwJscContext alloc] init];
        handle.context = context;
        return (__bridge_retained fw_jsc_context_t *)handle;
    }
}

void fw_jsc_context_release(fw_jsc_context_t *context) {
    if (!context) {
        return;
    }
    @autoreleasepool {
        FwJscContext *handle = (__bridge_transfer FwJscContext *)context;
        handle.context = nil;
    }
}

fw_jsc_gc_owner_t *fw_jsc_gc_owner_create(fw_jsc_context_t *context) {
    if (!context) {
        return NULL;
    }
    @autoreleasepool {
        FwJscContext *contextHandle = (__bridge FwJscContext *)context;
        FwJscGcOwner *handle = [[FwJscGcOwner alloc] init];
        handle.context = contextHandle.context;
        handle.owner = [[NSObject alloc] init];
        handle.key = [NSString
            stringWithFormat:@"__formal_web_gc_owner_%llu", (unsigned long long)++g_owner_counter];
        // Bridging the owner into the JS object graph is what lets
        // JavaScriptCore associate the managed references with it.
        handle.context[handle.key] = handle.owner;
        return (__bridge_retained fw_jsc_gc_owner_t *)handle;
    }
}

void fw_jsc_gc_owner_release(fw_jsc_gc_owner_t *owner) {
    if (!owner) {
        return;
    }
    @autoreleasepool {
        FwJscGcOwner *handle = (__bridge_transfer FwJscGcOwner *)owner;
        handle.context[handle.key] = nil;
        handle.owner = nil;
        handle.context = nil;
    }
}

fw_jsc_platform_object_t *fw_jsc_platform_object_create(
    fw_jsc_context_t *context, void *data)
{
    if (!context || !data) {
        return NULL;
    }
    @autoreleasepool {
        FwJscContext *contextHandle = (__bridge FwJscContext *)context;
        FwJscPlatformObject *holder = [[FwJscPlatformObject alloc] init];
        holder.data = data;
        holder.context = contextHandle.context;
        // Export the holder: this creates the JS object and puts the holder
        // into JavaScriptCore's Objective-C object graph, which is what makes
        // it usable as a managed-reference owner.
        JSValue *exported = [JSValue valueWithObject:holder inContext:contextHandle.context];
        if (!exported) {
            return NULL;
        }
        holder.jsObject = exported.JSValueRef;
        return (__bridge_retained fw_jsc_platform_object_t *)holder;
    }
}

JSObjectRef fw_jsc_platform_object_js_object(
    fw_jsc_context_t *context, fw_jsc_platform_object_t *holder)
{
    if (!context || !holder) {
        return NULL;
    }
    @autoreleasepool {
        FwJscContext *contextHandle = (__bridge FwJscContext *)context;
        FwJscPlatformObject *holderHandle = (__bridge FwJscPlatformObject *)holder;
        JSValue *exported = [JSValue valueWithObject:holderHandle inContext:contextHandle.context];
        if (!exported) {
            return NULL;
        }
        return exported.JSValueRef;
    }
}

void fw_jsc_platform_object_release(fw_jsc_platform_object_t *holder) {
    if (!holder) {
        return;
    }
    // The holder is kept alive by its JS wrapper as long as that wrapper is
    // reachable; this only drops the +1 from `fw_jsc_platform_object_create`.
    (void)(__bridge_transfer FwJscPlatformObject *)holder;
}

fw_jsc_managed_value_t *fw_jsc_managed_value_create(
    fw_jsc_context_t *context, JSValueRef value, void *owner)
{
    if (!context || !value) {
        return NULL;
    }
    @autoreleasepool {
        FwJscContext *contextHandle = (__bridge FwJscContext *)context;
        JSValue *jsValue = [JSValue valueWithJSValueRef:value inContext:contextHandle.context];
        if (!jsValue) {
            return NULL;
        }
        JSManagedValue *managed;
        if (owner) {
            id ownerObject = (__bridge id)owner;
            id managedOwner = [ownerObject respondsToSelector:@selector(managedOwner)]
                ? [ownerObject managedOwner]
                : ownerObject;
            managed = [JSManagedValue managedValueWithValue:jsValue andOwner:managedOwner];
        } else {
            managed = [JSManagedValue managedValueWithValue:jsValue];
        }
        if (!managed) {
            return NULL;
        }
        FwJscManagedValue *handle = [[FwJscManagedValue alloc] init];
        handle.managed = managed;
        handle.context = contextHandle.context;
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
