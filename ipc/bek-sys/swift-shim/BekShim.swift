// BekShim.swift — C-callable bridge over BrowserEngineKit's Swift-only API.
//
// BrowserEngineKit's extension-process objects (NetworkingProcess,
// WebContentProcess, RenderingProcess) are Swift structs with async
// initializers and throwing methods — there is no C entry point. This file
// wraps them in `@_cdecl` functions that hand the Rust side either an opaque
// process handle or a raw `xpc_connection_t` (which `xpc-sys` wraps
// unmodified via `XpcConnection::from_raw`).
//
// The shim is compiled by `build.rs` with the real BrowserEngineKit module
// from the iOS / iOS Simulator SDKs. A compile error here means the Rust
// declarations in `shim.rs` (or the `ipc` abstraction built on them) do not
// match BrowserEngineKit's actual API — that failure is the design check.
//
// Handle model: every process is boxed as a `ProcessBox` (a tagged union in
// disguise — a retained Swift object holding the concrete process struct).
// Rust keeps an opaque `*mut c_void` to the box plus the `kind` it was
// launched with; Swift downcasts through the box, so no other process type
// knowledge crosses the FFI boundary.

import BrowserEngineKit
import Foundation
import XPC

/// Kind discriminants shared with `BekProcessKind` in the Rust crate.
/// Ordering is load-bearing: the Rust enum must match it.
enum BekKind {
    static let networking: Int32 = 0
    static let rendering: Int32 = 1
    static let webContent: Int32 = 2
}

/// Capability discriminants shared with the `bek` backend's `ProcessCapability`.
enum BekCapability {
    static let foreground: Int32 = 0
    static let background: Int32 = 1
    static let suspended: Int32 = 2
}

/// Heap box holding one launched extension process, typed by `kind`.
/// Retained by the Rust side for the lifetime of the `ExtensionHandle`.
final class ProcessBox {
    let kind: Int32
    let process: Any

    init(kind: Int32, process: Any) {
        self.kind = kind
        self.process = process
    }

    func invalidate() {
        switch kind {
        case BekKind.networking:
            (process as! NetworkingProcess).invalidate()
        case BekKind.rendering:
            (process as! RenderingProcess).invalidate()
        case BekKind.webContent:
            (process as! WebContentProcess).invalidate()
        default:
            break
        }
    }

    func makeConnection() throws -> xpc_connection_t {
        switch kind {
        case BekKind.networking:
            return try (process as! NetworkingProcess).makeLibXPCConnection()
        case BekKind.rendering:
            return try (process as! RenderingProcess).makeLibXPCConnection()
        case BekKind.webContent:
            return try (process as! WebContentProcess).makeLibXPCConnection()
        default:
            throw NSError(
                domain: "com.formal-web.bek", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "unknown process kind"])
        }
    }

    /// Hand the underlying connection to C with one ownership retained on
    /// Rust's behalf (the `XpcConnection::from_raw` wrapper releases it on
    /// drop). The connection is an ARC-managed `OS_xpc_object` in Swift, so
    /// the ownership is transferred with ARC's retain: `passRetained` adds a
    /// reference that survives the ARC temporary releasing at scope exit,
    /// leaving exactly one owned reference for the Rust side.
    func takeConnection() throws -> UnsafeMutableRawPointer {
        let connection: xpc_connection_t = try makeConnection()
        return Unmanaged.passRetained(connection as AnyObject).toOpaque()
    }

    func grant(_ capability: ProcessCapability) throws -> ProcessCapability.Grant {
        switch kind {
        case BekKind.networking:
            return try (process as! NetworkingProcess).grantCapability(capability)
        case BekKind.rendering:
            return try (process as! RenderingProcess).grantCapability(capability)
        case BekKind.webContent:
            return try (process as! WebContentProcess).grantCapability(capability)
        default:
            throw NSError(
                domain: "com.formal-web.bek", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "unknown process kind"])
        }
    }
}

/// Heap box holding a live `ProcessCapability.Grant`, retained by the Rust
/// side until the grant is invalidated.
final class GrantBox {
    let grant: ProcessCapability.Grant

    init(_ grant: ProcessCapability.Grant) {
        self.grant = grant
    }
}

/// Malloc'd C string copy of an error description, freed by
/// `bek_free_string`. Kept alive across the completion callback only.
func copyErrorMessage(_ error: Error) -> UnsafeMutablePointer<CChar>? {
    let description = (error as NSError).localizedDescription
    return strdup(description)
}

// ── launch ────────────────────────────────────────────────────────────────

/// Launch (or find) an extension process. The launch itself is asynchronous
/// on BrowserEngineKit's side; the completion fires on a Swift task once the
/// process object exists. `on_interrupt` fires if the OS kills the process.
@_cdecl("bek_launch_process")
public func bekLaunchProcess(
    kind: Int32,
    bundleID: UnsafePointer<CChar>?,
    onInterrupt: @escaping @convention(c) () -> Void,
    onComplete: @escaping @convention(c) (UnsafeMutableRawPointer?, UnsafeMutableRawPointer?, UnsafeMutableRawPointer?) -> Void,
    context: UnsafeMutableRawPointer?
) {
    let bundle = bundleID.map { String(cString: $0) }
    Task {
        do {
            let process: Any
            switch kind {
            case BekKind.networking:
                process = try await NetworkingProcess(
                    bundleIdentifier: bundle, onInterruption: { onInterrupt() })
            case BekKind.rendering:
                process = try await RenderingProcess(
                    bundleIdentifier: bundle, onInterruption: { onInterrupt() })
            case BekKind.webContent:
                process = try await WebContentProcess(
                    bundleIdentifier: bundle, onInterruption: { onInterrupt() })
            default:
                onComplete(nil, nil, context)
                return
            }
            let box = ProcessBox(kind: kind, process: process)
            let handle = Unmanaged.passRetained(box).toOpaque()
            onComplete(handle, nil, context)
        } catch {
            onComplete(nil, copyErrorMessage(error), context)
        }
    }
}

/// Release the error string allocated by `copyErrorMessage` (malloc'd via
/// `strdup`, so freed with `free`).
@_cdecl("bek_free_string")
public func bekFreeString(_ pointer: UnsafeMutablePointer<CChar>?) {
    if let pointer {
        free(pointer)
    }
}

// ── connection / capability / lifecycle on a launched process ─────────────

/// Create the libxpc connection to a launched extension process. The
/// returned pointer is a +1 `xpc_connection_t`; the Rust side wraps it with
/// `xpc_sys::XpcConnection::from_raw` unchanged.
@_cdecl("bek_process_make_xpc_connection")
public func bekProcessMakeXpcConnection(
    handle: UnsafeMutableRawPointer,
    kind: Int32,
    errorMessage: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    let box = Unmanaged<ProcessBox>.fromOpaque(handle).takeUnretainedValue()
    guard box.kind == kind else { return nil }
    do {
        let connection = try box.takeConnection()
        return connection
    } catch {
        errorMessage?.pointee = copyErrorMessage(error)
        return nil
    }
}

@_cdecl("bek_process_grant_capability")
public func bekProcessGrantCapability(
    handle: UnsafeMutableRawPointer,
    kind: Int32,
    capability: Int32,
    errorMessage: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    let box = Unmanaged<ProcessBox>.fromOpaque(handle).takeUnretainedValue()
    guard box.kind == kind else { return nil }
    let processCapability: ProcessCapability
    switch capability {
    case BekCapability.foreground:
        processCapability = .foreground
    case BekCapability.background:
        processCapability = .background
    case BekCapability.suspended:
        processCapability = .suspended
    default:
        return nil
    }
    do {
        let grant = try box.grant(processCapability)
        let grantBox = GrantBox(grant)
        return Unmanaged.passRetained(grantBox).toOpaque()
    } catch {
        errorMessage?.pointee = copyErrorMessage(error)
        return nil
    }
}

/// Stop the extension process (releases the box held by the handle).
@_cdecl("bek_process_invalidate")
public func bekProcessInvalidate(handle: UnsafeMutableRawPointer, kind: Int32) {
    let box = Unmanaged<ProcessBox>.fromOpaque(handle).takeRetainedValue()
    guard box.kind == kind else { return }
    box.invalidate()
}

/// Invalidate a capability grant (releases the grant box held by the handle).
@_cdecl("bek_grant_invalidate")
public func bekGrantInvalidate(handle: UnsafeMutableRawPointer) {
    let grantBox = Unmanaged<GrantBox>.fromOpaque(handle).takeRetainedValue()
    grantBox.grant.invalidate()
}
