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

// The realm-lifetime owner object, exported to JavaScript so JavaScriptCore's
// managed-reference scan can resolve it.
@protocol FwJscRealmOwnerExport <JSExport>
@end

@interface FwJscRealmOwner : NSObject <FwJscRealmOwnerExport>
@end

@implementation FwJscRealmOwner
@end

// A realm-lifetime owner, bridged into the JS graph as a property of the
// context's global object.
@interface FwJscGcOwner : NSObject
@property (nonatomic, strong) JSContext *context;
@property (nonatomic, strong) FwJscRealmOwner *owner;
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
@protocol FwJscPlatformObjectExport <JSExport>
@end

@interface FwJscPlatformObject : NSObject <FwJscPlatformObjectExport>
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

// A retained `JSManagedValue` plus the context it belongs to.  When `owner`
// is set the managed value is also registered with the virtual machine as a
// managed reference; `dealloc` removes that registration, which is what keeps
// the machine's managed-reference set free of dangling entries.
@interface FwJscManagedValue : NSObject
@property (nonatomic, strong) JSManagedValue *managed;
@property (nonatomic, strong) JSContext *context;
// Weak: the virtual machine tracks the owner's reachability; retaining it
// here would keep a collected platform holder alive through its Rust cells.
@property (nonatomic, weak) id owner;
@end

@implementation FwJscManagedValue
- (void)dealloc {
    if (self.managed && self.owner) {
        [self.context.virtualMachine removeManagedReference:self.managed
                                                   withOwner:self.owner];
    }
}
@end

// Build a managed value for `jsValue`.  `owner` (already resolved from a
// platform holder or realm anchor) is nil for a weak reference.
static FwJscManagedValue *fw_jsc_make_managed_value(
    JSContext *context, JSValue *jsValue, id owner)
{
    JSManagedValue *managed = [JSManagedValue managedValueWithValue:jsValue];
    if (!managed) {
        return nil;
    }
    FwJscManagedValue *handle = [[FwJscManagedValue alloc] init];
    handle.managed = managed;
    handle.context = context;
    if (owner) {
        handle.owner = owner;
        [context.virtualMachine addManagedReference:managed withOwner:owner];
    }
    return handle;
}

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
        handle.owner = [[FwJscRealmOwner alloc] init];
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
        id managedOwner = nil;
        if (owner) {
            id ownerObject = (__bridge id)owner;
            managedOwner = [ownerObject respondsToSelector:@selector(managedOwner)]
                ? [ownerObject managedOwner]
                : ownerObject;
        }
        FwJscManagedValue *handle =
            fw_jsc_make_managed_value(contextHandle.context, jsValue, managedOwner);
        if (!handle) {
            return NULL;
        }
        return (__bridge_retained fw_jsc_managed_value_t *)handle;
    }
}

fw_jsc_managed_value_t *fw_jsc_managed_value_retain(fw_jsc_managed_value_t *managed) {
    if (!managed) {
        return NULL;
    }
    @autoreleasepool {
        FwJscManagedValue *handle = (__bridge FwJscManagedValue *)managed;
        JSValue *value = handle.managed.value;
        if (!value) {
            // The value has already been collected; a clone is an empty
            // handle that reports `None`.
            FwJscManagedValue *copy = [[FwJscManagedValue alloc] init];
            return (__bridge_retained fw_jsc_managed_value_t *)copy;
        }
        // Each handle registers (and later removes) its own managed reference,
        // so add/remove stay balanced under any virtual-machine bookkeeping.
        FwJscManagedValue *copy =
            fw_jsc_make_managed_value(handle.context, value, handle.owner);
        return (__bridge_retained fw_jsc_managed_value_t *)copy;
    }
}

void fw_jsc_managed_value_release(fw_jsc_managed_value_t *managed) {
    if (!managed) {
        return;
    }
    @autoreleasepool {
        // Dealloc removes the managed reference; let ARC run it rather than
        // clearing fields here.
        (void)(__bridge_transfer FwJscManagedValue *)managed;
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
