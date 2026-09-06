//! The AppKit application: NSApplication lifecycle, windows, a native
//! AppKit chrome (tab strip and address field), event routing, and
//! display-link pacing.
//!
//! The app runs an `NSApplication` with the web content in a layer-hosting
//! view whose layer `contents` is set to the shared IOSurface from the
//! graphics process (the zero-copy blit). The chrome is native AppKit
//! controls: a tab strip of rounded tab cells and an editable `NSTextField`
//! address bar. A `CVDisplayLink` paces animated content: each tick
//! requests the next frame via `WebviewProvider::frame_needed` at the
//! display refresh rate, and the link runs only while the composed scene is
//! animating.

mod automation_host;

use crate::events::{EventLoopEmbedder, FormalWebUserEvent, UserEventSink};
use crate::input;
use crate::platform::{
    normalize_browser_destination, read_clipboard_text, startup_destination_url,
    update_window_viewport_snapshot, write_clipboard_text,
};
use crate::window::{new_layer_hosted_view, present_shared_surface};
use automation::AutomationController;
use block2::RcBlock;
use keyboard_types::Modifiers as KeyboardModifiers;
use log::{debug, error, info};
use objc2::define_class;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBitmapImageFileType,
    NSBitmapImageRepPropertyKey, NSButton, NSButtonType, NSCellImagePosition, NSColor,
    NSControlTextEditingDelegate, NSFont, NSImage, NSImageSymbolConfiguration, NSImageSymbolScale,
    NSLineBreakMode, NSMenu, NSMenuItem, NSTextField, NSTextFieldBezelStyle, NSTextFieldDelegate,
    NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode, NSToolbarFlexibleSpaceItemIdentifier,
    NSToolbarItem, NSToolbarItemGroup, NSToolbarItemGroupControlRepresentation,
    NSToolbarItemIdentifier, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
    NSWindowTitleVisibility, NSWindowToolbarStyle,
};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSFontWeightMedium};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{CVDisplayLink, CVOptionFlags, CVReturn, CVTimeStamp, kCVReturnSuccess};
use objc2_foundation::NSInteger;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDictionary, NSNotification, NSObject, NSObjectProtocol, NSPoint,
    NSRange, NSRect, NSSet, NSSize, NSString, ns_string,
};
use objc2_io_surface::IOSurfaceRef;
use objc2_quartz_core::{CAAutoresizingMask, CALayer};
use std::cell::UnsafeCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;
use verification::TraceSender;
use webview::WebviewProvider;
use webview::{
    BlitzPointerEvent, BlitzPointerId, BlitzWheelEvent, ColorScheme, CompositingLayerId,
    LayerFrame, MouseEventButtons, NavigationCompleted, NavigationCompletion, PointerCoords,
    SurfaceFrame, UiEvent, WebviewId, deallocate_mach_port,
};

const INITIAL_WINDOW_WIDTH: f64 = 1200.0;
const INITIAL_WINDOW_HEIGHT: f64 = 800.0;

/// The tab strip and address field live in the window's native toolbar row
/// as custom-view items; these are the sizes of the hosted views.
const ADDRESS_FIELD_HEIGHT: f64 = 28.0;
const TAB_STRIP_HEIGHT: f64 = 36.0;
/// The narrowest the tab-strip and address-field toolbar items may shrink.
const TOOLBAR_ITEM_MIN_WIDTH: f64 = 240.0;
/// The widest the address-field toolbar item may grow.
const ADDRESS_FIELD_MAX_WIDTH: f64 = 600.0;
const TAB_BUTTON_WIDTH: f64 = 160.0;
/// The narrowest a tab cell may shrink before overflowing the strip.
const MIN_TAB_CELL_WIDTH: f64 = 90.0;
/// The gap between adjacent tab cells.
const TAB_CELL_GAP: f64 = 2.0;
const TAB_BUTTON_HEIGHT: f64 = 26.0;
/// The corner radius of the tab pill (the cell's background).
const TAB_CORNER_RADIUS: f64 = 8.0;
/// The close (×) button inside each tab cell.
const TAB_CLOSE_BUTTON_WIDTH: f64 = 22.0;
/// Points inside this margin of the content view's bottom/side edges belong
/// to the window frame's resize handles; the initial mouse-down there must
/// reach AppKit's resize tracking loop.
const WINDOW_RESIZE_MARGIN: f64 = 6.0;
/// Points inside this radius of the content view's bottom corners belong to
/// the frame's grow box (larger than the edge margin).
const WINDOW_RESIZE_CORNER_RADIUS: f64 = 16.0;

// ── App delegate ───────────────────────────────────────────────────────────

/// The window delegate ivars: a pointer to the app state, confined to the
/// main thread.
#[repr(C)]
struct DelegateIvars {
    app: UnsafeCell<*mut MacApp>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the delegate is
    // retained by the app and released before the app state is dropped.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    #[name = "FormalWebAppDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    // SAFETY: `NSWindowDelegate` has no safety requirements; the methods
    // only touch main-thread-confined app state.
    unsafe impl NSWindowDelegate for Delegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, notification: &NSNotification) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.window_will_close(notification);
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, notification: &NSNotification) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.window_did_resize(notification);
        }

        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing_properties(&self, notification: &NSNotification) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.window_did_change_backing_properties(notification);
        }

        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, notification: &NSNotification) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.window_did_become_key(notification);
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, notification: &NSNotification) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.window_did_resign_key(notification);
        }

        #[unsafe(method(windowDidMiniaturize:))]
        fn window_did_miniaturize(&self, notification: &NSNotification) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.window_did_miniaturize(notification);
        }

        #[unsafe(method(windowDidDeminiaturize:))]
        fn window_did_deminiaturize(&self, _notification: &NSNotification) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.start_display_link_if_animating();
        }
    }

    // SAFETY: `NSToolbarDelegate` has no safety requirements; the methods
    // only touch main-thread-confined app state. The protocol's generated
    // method names are the raw selectors (camel case).
    #[allow(non_snake_case)]
    unsafe impl NSToolbarDelegate for Delegate {
        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn toolbarDefaultItemIdentifiers(
            &self,
            _toolbar: &NSToolbar,
        ) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            Self::toolbar_default_identifiers()
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn toolbarAllowedItemIdentifiers(
            &self,
            _toolbar: &NSToolbar,
        ) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            Self::toolbar_default_identifiers()
        }

        #[unsafe(method_id(toolbarImmovableItemIdentifiers:))]
        fn toolbarImmovableItemIdentifiers(
            &self,
            _toolbar: &NSToolbar,
        ) -> Retained<NSSet<NSToolbarItemIdentifier>> {
            // The address field cannot be removed or moved: a browser
            // needs it.
            NSSet::from_retained_slice(&[NSString::from_str("address")])
        }

        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn toolbar_itemForItemIdentifier_willBeInsertedIntoToolbar(
            &self,
            toolbar: &NSToolbar,
            item_identifier: &NSToolbarItemIdentifier,
            flag: bool,
        ) -> Option<Retained<NSToolbarItem>> {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.make_toolbar_item(toolbar, item_identifier, flag)
        }
    }

    // SAFETY: `NSControlTextEditingDelegate` has no safety requirements;
    // the methods only touch main-thread-confined app state.
    unsafe impl NSControlTextEditingDelegate for Delegate {
        #[unsafe(method(controlTextDidBeginEditing:))]
        fn control_text_did_begin_editing(&self, _notification: &NSNotification) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.address_field_begin_editing();
        }

        #[unsafe(method(controlTextDidEndEditing:))]
        fn control_text_did_end_editing(&self, _notification: &NSNotification) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.address_field_end_editing();
        }
    }

    // SAFETY: `NSTextFieldDelegate` adds no required methods beyond its
    // superprotocol; the field's delegate must conform to it for
    // `setDelegate:`.
    unsafe impl NSTextFieldDelegate for Delegate {}

    impl Delegate {
        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.post_exit();
        }

        #[unsafe(method(switchTab:))]
        fn switch_tab(&self, sender: Option<&AnyObject>) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            let Some(sender) = sender else { return };
            // SAFETY: the sender is one of the tab buttons, which respond
            // to `tag`.
            let tag: NSInteger = unsafe { msg_send![sender, tag] };
            app.action_switch_tab(tag.max(0) as usize);
        }

        #[unsafe(method(newTab:))]
        fn new_tab(&self, _sender: Option<&AnyObject>) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.action_new_tab();
        }

        #[unsafe(method(navigate:))]
        fn navigate(&self, sender: Option<&AnyObject>) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            let Some(sender) = sender else { return };
            // SAFETY: the sender is the address field, which responds to
            // `stringValue`.
            let value: Retained<NSString> = unsafe { msg_send![sender, stringValue] };
            app.action_navigate(value.to_string());
        }

        #[unsafe(method(newWindow:))]
        fn new_window(&self, _sender: Option<&AnyObject>) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.action_new_window();
        }

        #[unsafe(method(closeTab:))]
        fn close_tab(&self, _sender: Option<&AnyObject>) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.action_close_tab();
        }

        #[unsafe(method(closeTabAt:))]
        fn close_tab_at(&self, sender: Option<&AnyObject>) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            let Some(sender) = sender else { return };
            // SAFETY: the sender is one of the tab close buttons, which
            // respond to `tag`.
            let tag: NSInteger = unsafe { msg_send![sender, tag] };
            app.action_close_tab_at(tag.max(0) as usize);
        }

        #[unsafe(method(closeWindow:))]
        fn close_window(&self, _sender: Option<&AnyObject>) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.action_close_window();
        }

        #[unsafe(method(reload:))]
        fn reload(&self, _sender: Option<&AnyObject>) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.action_reload();
        }

        #[unsafe(method(focusAddress:))]
        fn focus_address(&self, _sender: Option<&AnyObject>) {
            let app = unsafe { &mut *(*self.ivars().app.get()) };
            app.action_focus_address();
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars {
            app: UnsafeCell::new(std::ptr::null_mut()),
        });
        // SAFETY: the superclass initializer has the correct signature.
        unsafe { msg_send![super(this), init] }
    }

    /// The ordered toolbar item identifiers: the joined back/forward
    /// control, the reload button, and the address field centered between
    /// two flexible spaces.
    fn toolbar_default_identifiers() -> Retained<NSArray<NSToolbarItemIdentifier>> {
        let back_forward = NSString::from_str("backForward");
        let reload = NSString::from_str("reload");
        let address = NSString::from_str("address");
        let new_tab = NSString::from_str("newTab");
        // The flexible-space item must be the system constant object, not
        // a string with the same content: AppKit matches space items by
        // identity.
        let flexible_space = unsafe { NSToolbarFlexibleSpaceItemIdentifier };
        // A flexible space on each side of the address field keeps it
        // horizontally centered (Safari-style). The new-tab button sits at
        // the trailing edge, in the toolbar rather than the tab strip. The
        // tab strip is not a toolbar item; it lives below the toolbar as
        // its own row.
        NSArray::from_slice(&[
            &back_forward,
            &reload,
            flexible_space,
            &address,
            flexible_space,
            &new_tab,
        ])
    }
}

// ── Tab cell ───────────────────────────────────────────────────────────────

/// The tab pill's fill: the resting state (inactive), the active tab's
/// fill, or the hover fill.
#[derive(Clone, Copy)]
enum TabPill {
    None,
    Active,
    Hover,
}

// ── Main-thread handle ─────────────────────────────────────────────────────

/// A raw pointer to the app state that is only ever dereferenced on the
/// main thread. The explicit `Send`/`Sync` impls let the handle travel
/// through dispatch blocks that run on the main queue.
#[derive(Clone, Copy)]
pub(crate) struct MainThreadHandle {
    app: NonNull<MacApp>,
}

// SAFETY: the pointer is only ever dereferenced on the main thread.
unsafe impl Send for MainThreadHandle {}
unsafe impl Sync for MainThreadHandle {}

impl MainThreadHandle {
    fn new(app: *mut MacApp) -> Self {
        Self {
            app: NonNull::new(app).expect("app pointer must be non-null"),
        }
    }

    /// The app pointer. A method call (rather than field access) so that
    /// closures capture the whole `Send` handle, not the raw field.
    pub(crate) fn app_ptr(self) -> *mut MacApp {
        self.app.as_ptr()
    }
}

// ── Display link ───────────────────────────────────────────────────────────

/// The CVDisplayLink context, kept alive for the app's lifetime and passed
/// to the C callback as user data.
struct DisplayLinkContext {
    handle: MainThreadHandle,
}

unsafe extern "C-unwind" fn display_link_callback(
    _link: NonNull<CVDisplayLink>,
    _now: NonNull<CVTimeStamp>,
    _output: NonNull<CVTimeStamp>,
    _flags: CVOptionFlags,
    _flags_out: NonNull<CVOptionFlags>,
    context: *mut c_void,
) -> CVReturn {
    let context = unsafe { &*context.cast::<DisplayLinkContext>() };
    let handle = context.handle;
    dispatch::Queue::main().exec_async(move || {
        let app = unsafe { &mut *handle.app_ptr() };
        app.display_link_tick();
    });
    kCVReturnSuccess
}

// ── State ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct WindowId(Uuid);

impl WindowId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Per-tab state.
struct TabState {
    pending_url: Option<String>,
    committed_url: Option<String>,
    /// The parsed title of the committed document, reported by the content
    /// process after parsing.
    page_title: Option<String>,
}

impl TabState {
    fn new() -> Self {
        Self {
            pending_url: None,
            committed_url: None,
            page_title: None,
        }
    }

    fn display_url(&self) -> String {
        self.pending_url
            .clone()
            .or_else(|| self.committed_url.clone())
            .unwrap_or_default()
    }
}

/// The latest composited surface for one webview: the zero-copy shared
/// IOSurface (the only delivery path on macOS) plus the metadata needed to
/// present it and to pace the display link.
struct SurfaceState {
    surface: CFRetained<IOSurfaceRef>,
    width: u32,
    /// The IOSurface's padded (64-multiple) width.
    padded_width: u32,
    animating: bool,
}

/// Per-window state: the NSWindow, the native chrome views, tabs, and
/// per-webview surfaces.
struct MacWindow {
    window: Retained<NSWindow>,
    /// The window's native toolbar, which hosts the tab strip and the
    /// address field as custom-view items.
    toolbar: Retained<NSToolbar>,
    tab_strip: Retained<NSVisualEffectView>,
    address_field: Retained<NSTextField>,
    /// One cell per tab: the container holding the label and close
    /// buttons and the pill background (drawn on its layer).
    tab_cells: Vec<Retained<NSView>>,
    tab_buttons: Vec<Retained<NSButton>>,
    tab_close_buttons: Vec<Retained<NSButton>>,
    /// The tab under the pointer, for re-applying the hover state when
    /// the strip is rebuilt.
    hovered_tab: Option<usize>,
    /// True while the address field is editing; the whole URL is selected
    /// only on the first focus of each editing session (Safari behavior).
    address_field_focused: bool,
    web_view: Retained<NSView>,
    /// The layer-hosting layer that presents the shared IOSurface; kept
    /// alive by the app.
    web_layer: Retained<objc2_quartz_core::CALayer>,
    tabs: HashMap<WebviewId, TabState>,
    tab_order: Vec<WebviewId>,
    active_tab: Option<WebviewId>,
    surfaces: HashMap<WebviewId, SurfaceState>,
    /// Sublayers under `web_layer`, one per child (non-root) compositing
    /// layer of the active webview's composition: cross-origin iframe
    /// navigables and `<video>` embed sites. The root navigable itself is
    /// presented on `web_layer`, not as one of these.
    sublayers: HashMap<CompositingLayerId, Retained<objc2_quartz_core::CALayer>>,
    keyboard_modifiers: KeyboardModifiers,
    buttons: MouseEventButtons,
    /// Content view size in points (the window's content area).
    content_size: (f64, f64),
    scale: f64,
    minimized: bool,
}

pub(crate) struct MacApp {
    mtm: MainThreadMarker,
    ns_app: Retained<NSApplication>,
    delegate: Retained<Delegate>,
    /// Installed right after the app struct is built (self-referential).
    handle: Option<MainThreadHandle>,
    windows: HashMap<WindowId, MacWindow>,
    active_window_id: Option<WindowId>,
    /// Script-opened traversables (window.open) whose navigation has not
    /// committed yet.  They are not shown as tabs until the commit arrives,
    /// so the intermediate about:blank document is never visible.
    pending_script_webviews: HashSet<WebviewId>,
    provider: Option<WebviewProvider>,
    display_link: Option<CFRetained<CVDisplayLink>>,
    display_link_context: Option<Box<DisplayLinkContext>>,
    display_link_running: bool,
    event_monitor: Option<Retained<AnyObject>>,
    automation: AutomationController,
    cdp_server: Option<automation::CdpServerHandle>,
    exiting: bool,
}

impl MacApp {
    // ── Entry point ────────────────────────────────────────────────────────

    fn run(
        mtm: MainThreadMarker,
        trace_sender: Option<TraceSender>,
        startup_url: Option<String>,
        window_title: Option<String>,
        cdp_port: Option<u16>,
    ) -> Result<(), String> {
        let ns_app = NSApplication::sharedApplication(mtm);
        ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

        let mut app = Self {
            mtm,
            ns_app: ns_app.clone(),
            delegate: Delegate::new(mtm),
            handle: None,
            windows: HashMap::new(),
            active_window_id: None,
            pending_script_webviews: HashSet::new(),
            provider: None,
            display_link: None,
            display_link_context: None,
            display_link_running: false,
            event_monitor: None,
            automation: AutomationController::default(),
            cdp_server: None,
            exiting: false,
        };
        let app_ptr = &mut app as *mut MacApp;
        app.handle = Some(MainThreadHandle::new(app_ptr));
        unsafe {
            *app.delegate.ivars().app.get() = app_ptr;
        }

        // The sink must be created before the user agent starts, so the
        // UA's initial events reach the app.
        let sink: Arc<dyn UserEventSink> = Arc::new(crate::events::MacEventSink {
            handle: app.handle.expect("app handle must be installed"),
        });
        // The CDP server runs on its own thread; it posts commands through a
        // clone of the sink and gates on `runtime_ready` (true here, cleared
        // when the run loop exits).
        let cdp_sink = sink.clone();
        let runtime_ready = Arc::new(AtomicBool::new(true));
        let embedder = Arc::new(EventLoopEmbedder::new(sink));
        let provider = WebviewProvider::new(embedder, trace_sender)?;
        app.provider = Some(provider);

        app.install_event_monitor()?;
        app.install_main_menu();
        app.create_display_link()?;

        let title = window_title.unwrap_or_else(|| String::from("formal-web"));
        let destination = startup_destination_url(startup_url.as_deref())
            .unwrap_or_else(|_| String::from("about:blank"));
        let window_id = app.create_window(&title, &destination)?;
        app.active_window_id = Some(window_id);

        // The CDP server runs on its own thread; each command is routed to
        // the app's main-thread event loop via the user-event sink. It is
        // only started when a port is requested, so a plain app launch is
        // unchanged.
        if let Some(port) = cdp_port {
            let sink_for_commands = cdp_sink.clone();
            let sink_for_exit = cdp_sink.clone();
            let ready = Arc::clone(&runtime_ready);
            let runtime = automation::automation_bridge(
                move |command| sink_for_commands.send(FormalWebUserEvent::Automation(command)),
                move || sink_for_exit.send(FormalWebUserEvent::Exit),
                move || ready.load(Ordering::Relaxed),
            );
            let server = automation::CdpServerHandle::start(port, runtime).map_err(|error| {
                format!("failed to start the CDP server for the AppKit embedder: {error}")
            })?;
            app.cdp_server = Some(server);
            info!(
                "[mac-embedder] CDP server listening on 127.0.0.1:{} (AppKit embedder)",
                port
            );
        }

        // Activate the app (required when launching unbundled) and start
        // the run loop; the display link and the event sink drive the rest.
        #[allow(deprecated)]
        ns_app.activateIgnoringOtherApps(true);

        info!("[mac-embedder] starting NSApplication run loop");
        ns_app.run();
        info!("[mac-embedder] NSApplication run loop ended");

        runtime_ready.store(false, Ordering::Relaxed);
        update_window_viewport_snapshot(None);
        Ok(())
    }

    fn post_exit(&mut self) {
        if self.exiting {
            return;
        }
        self.exiting = true;
        info!("[mac-embedder] exiting");
        self.stop_display_link();
        if let Some(monitor) = self.event_monitor.take() {
            // SAFETY: the monitor object is the one returned by the
            // addLocalMonitor call.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
        self.cdp_server = None;
        self.provider = None;
        update_window_viewport_snapshot(None);
        self.ns_app.terminate(None);
    }

    // ── Display link ───────────────────────────────────────────────────────

    fn create_display_link(&mut self) -> Result<(), String> {
        let mut link_ptr: *mut CVDisplayLink = std::ptr::null_mut();
        #[allow(deprecated)]
        let ret =
            unsafe { CVDisplayLink::create_with_active_cg_displays(NonNull::from(&mut link_ptr)) };
        if ret != kCVReturnSuccess || link_ptr.is_null() {
            return Err(format!("failed to create CVDisplayLink (CVReturn {ret})"));
        }
        // SAFETY: the display link was just created with a +1 retain.
        let link = unsafe { CFRetained::from_raw(NonNull::new(link_ptr).unwrap()) };
        let context = Box::new(DisplayLinkContext {
            handle: self.handle.expect("app handle must be installed"),
        });
        let context_ptr: *mut c_void = &*context as *const DisplayLinkContext as *mut c_void;
        #[allow(deprecated)]
        let ret = unsafe { link.set_output_callback(Some(display_link_callback), context_ptr) };
        if ret != kCVReturnSuccess {
            return Err(format!(
                "failed to set CVDisplayLink output callback (CVReturn {ret})"
            ));
        }
        self.display_link_context = Some(context);
        self.display_link = Some(link);
        Ok(())
    }

    fn start_display_link(&mut self) {
        if self.display_link_running {
            return;
        }
        let visible = self
            .windows
            .values()
            .any(|window_state| !window_state.minimized);
        if !visible {
            return;
        }
        if let Some(link) = &self.display_link {
            #[allow(deprecated)]
            let started = link.start() == kCVReturnSuccess;
            if started {
                self.display_link_running = true;
                info!("[mac-embedder] display link started");
            }
        }
    }

    fn start_display_link_if_animating(&mut self) {
        let animating = self.windows.values().any(|window_state| {
            window_state
                .active_tab
                .and_then(|webview_id| window_state.surfaces.get(&webview_id))
                .is_some_and(|surface| surface.animating)
        });
        if animating {
            self.start_display_link();
        }
    }

    fn stop_display_link(&mut self) {
        if !self.display_link_running {
            return;
        }
        if let Some(link) = &self.display_link {
            #[allow(deprecated)]
            link.stop();
        }
        self.display_link_running = false;
        info!("[mac-embedder] display link stopped");
    }

    fn display_link_tick(&mut self) {
        if self.exiting {
            return;
        }
        // Request the next frame at display cadence. The UA gates actual
        // rendering on its queued rendering opportunities, so an idle scene
        // is not re-rendered.
        if let Some(window_id) = self.active_window_id
            && let Some(webview_id) = self.windows.get(&window_id).and_then(|w| w.active_tab)
            && let Some(provider) = &self.provider
            && let Err(error) = provider.frame_needed(webview_id)
        {
            error!("[mac-embedder] frame needed: {error}");
        }
    }

    // ── Menu ──────────────────────────────────────────────────────────────

    fn install_main_menu(&mut self) {
        let mtm = self.mtm;
        let main_menu = NSMenu::new(mtm);

        // App menu: about, hide, and quit. The top-level item title is the
        // app name shown next to the Apple menu glyph.
        let app_menu_item = NSMenuItem::new(mtm);
        app_menu_item.setTitle(ns_string!("formal-web"));
        let app_menu = NSMenu::new(mtm);
        app_menu.addItem(&Self::make_menu_item(
            mtm,
            "About formal-web",
            None,
            NSEventModifierFlags::Command,
            Some(sel!(orderFrontStandardAboutPanel:)),
            None,
        ));
        app_menu.addItem(&NSMenuItem::separatorItem(mtm));
        app_menu.addItem(&Self::make_menu_item(
            mtm,
            "Hide formal-web",
            Some("h"),
            NSEventModifierFlags::Command,
            Some(sel!(hide:)),
            None,
        ));
        app_menu.addItem(&Self::make_menu_item(
            mtm,
            "Hide Others",
            Some("h"),
            NSEventModifierFlags::Command | NSEventModifierFlags::Option,
            Some(sel!(hideOtherApplications:)),
            None,
        ));
        app_menu.addItem(&Self::make_menu_item(
            mtm,
            "Show All",
            None,
            NSEventModifierFlags::Command,
            Some(sel!(unhideAllApplications:)),
            None,
        ));
        app_menu.addItem(&NSMenuItem::separatorItem(mtm));
        app_menu.addItem(&Self::make_menu_item(
            mtm,
            "Quit formal-web",
            Some("q"),
            NSEventModifierFlags::Command,
            Some(sel!(quit:)),
            Some(&*self.delegate),
        ));
        app_menu_item.setSubmenu(Some(&app_menu));
        main_menu.addItem(&app_menu_item);

        // File: window and tab management, location entry, and closing.
        main_menu.addItem(&Self::make_menu_with_items(
            mtm,
            "File",
            &[
                Self::make_menu_item(
                    mtm,
                    "New Window",
                    Some("n"),
                    NSEventModifierFlags::Command,
                    Some(sel!(newWindow:)),
                    Some(&*self.delegate),
                ),
                Self::make_menu_item(
                    mtm,
                    "New Tab",
                    Some("t"),
                    NSEventModifierFlags::Command,
                    Some(sel!(newTab:)),
                    Some(&*self.delegate),
                ),
                Self::make_menu_item(
                    mtm,
                    "Open Location…",
                    Some("l"),
                    NSEventModifierFlags::Command,
                    Some(sel!(focusAddress:)),
                    Some(&*self.delegate),
                ),
                NSMenuItem::separatorItem(mtm),
                Self::make_menu_item(
                    mtm,
                    "Close Tab",
                    Some("w"),
                    NSEventModifierFlags::Command,
                    Some(sel!(closeTab:)),
                    Some(&*self.delegate),
                ),
                Self::make_menu_item(
                    mtm,
                    "Close Window",
                    Some("w"),
                    NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
                    Some(sel!(closeWindow:)),
                    Some(&*self.delegate),
                ),
            ],
        ));

        // Edit: the standard text-edit commands. With no target they are
        // dispatched through the responder chain, so they operate on the
        // address field's editor while it is being edited and stay
        // disabled otherwise.
        main_menu.addItem(&Self::make_menu_with_items(
            mtm,
            "Edit",
            &[
                Self::make_menu_item(
                    mtm,
                    "Undo",
                    Some("z"),
                    NSEventModifierFlags::Command,
                    Some(sel!(undo:)),
                    None,
                ),
                Self::make_menu_item(
                    mtm,
                    "Redo",
                    Some("z"),
                    NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
                    Some(sel!(redo:)),
                    None,
                ),
                NSMenuItem::separatorItem(mtm),
                Self::make_menu_item(
                    mtm,
                    "Cut",
                    Some("x"),
                    NSEventModifierFlags::Command,
                    Some(sel!(cut:)),
                    None,
                ),
                Self::make_menu_item(
                    mtm,
                    "Copy",
                    Some("c"),
                    NSEventModifierFlags::Command,
                    Some(sel!(copy:)),
                    None,
                ),
                Self::make_menu_item(
                    mtm,
                    "Paste",
                    Some("v"),
                    NSEventModifierFlags::Command,
                    Some(sel!(paste:)),
                    None,
                ),
                Self::make_menu_item(
                    mtm,
                    "Select All",
                    Some("a"),
                    NSEventModifierFlags::Command,
                    Some(sel!(selectAll:)),
                    None,
                ),
            ],
        ));

        // View: reload and full screen.
        main_menu.addItem(&Self::make_menu_with_items(
            mtm,
            "View",
            &[
                Self::make_menu_item(
                    mtm,
                    "Reload",
                    Some("r"),
                    NSEventModifierFlags::Command,
                    Some(sel!(reload:)),
                    Some(&*self.delegate),
                ),
                NSMenuItem::separatorItem(mtm),
                Self::make_menu_item(
                    mtm,
                    "Enter Full Screen",
                    Some("f"),
                    NSEventModifierFlags::Command | NSEventModifierFlags::Control,
                    Some(sel!(toggleFullScreen:)),
                    None,
                ),
            ],
        ));

        // History: session history is not implemented yet, so the standard
        // navigation items exist but stay disabled.
        let history_menu_item = NSMenuItem::new(mtm);
        history_menu_item.setTitle(ns_string!("History"));
        let history_menu = NSMenu::new(mtm);
        let back_item = Self::make_menu_item(
            mtm,
            "Back",
            Some("["),
            NSEventModifierFlags::Command,
            Some(sel!(goBack:)),
            None,
        );
        back_item.setEnabled(false);
        history_menu.addItem(&back_item);
        let forward_item = Self::make_menu_item(
            mtm,
            "Forward",
            Some("]"),
            NSEventModifierFlags::Command,
            Some(sel!(goForward:)),
            None,
        );
        forward_item.setEnabled(false);
        history_menu.addItem(&forward_item);
        history_menu.addItem(&NSMenuItem::separatorItem(mtm));
        let show_all_item = Self::make_menu_item(
            mtm,
            "Show All History",
            Some("y"),
            NSEventModifierFlags::Command,
            Some(sel!(showAllHistory:)),
            None,
        );
        show_all_item.setEnabled(false);
        history_menu.addItem(&show_all_item);
        history_menu_item.setSubmenu(Some(&history_menu));
        main_menu.addItem(&history_menu_item);

        // Window: the standard window commands, dispatched through the
        // responder chain to the key window (NSWindow implements
        // performMiniaturize:/performZoom:).
        main_menu.addItem(&Self::make_menu_with_items(
            mtm,
            "Window",
            &[
                Self::make_menu_item(
                    mtm,
                    "Minimize",
                    Some("m"),
                    NSEventModifierFlags::Command,
                    Some(sel!(performMiniaturize:)),
                    None,
                ),
                Self::make_menu_item(
                    mtm,
                    "Zoom",
                    None,
                    NSEventModifierFlags::Command,
                    Some(sel!(performZoom:)),
                    None,
                ),
                NSMenuItem::separatorItem(mtm),
                Self::make_menu_item(
                    mtm,
                    "Bring All to Front",
                    None,
                    NSEventModifierFlags::Command,
                    Some(sel!(arrangeInFront:)),
                    None,
                ),
            ],
        ));

        // Help: no help book yet, so the item stays disabled.
        let help_menu_item = NSMenuItem::new(mtm);
        help_menu_item.setTitle(ns_string!("Help"));
        let help_menu = NSMenu::new(mtm);
        let help_item = Self::make_menu_item(
            mtm,
            "formal-web Help",
            None,
            NSEventModifierFlags::Command,
            None,
            None,
        );
        help_item.setEnabled(false);
        help_menu.addItem(&help_item);
        help_menu_item.setSubmenu(Some(&help_menu));
        main_menu.addItem(&help_menu_item);

        self.ns_app.setMainMenu(Some(&main_menu));
    }

    /// A menu item with an optional key equivalent and action. With a nil
    /// target the action is dispatched through the responder chain, the
    /// standard AppKit mechanism for edit/window commands.
    fn make_menu_item(
        mtm: MainThreadMarker,
        title: &str,
        key_equivalent: Option<&str>,
        modifiers: NSEventModifierFlags,
        action: Option<Sel>,
        target: Option<&AnyObject>,
    ) -> Retained<NSMenuItem> {
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(title));
        if let Some(key_equivalent) = key_equivalent {
            item.setKeyEquivalent(&NSString::from_str(key_equivalent));
            item.setKeyEquivalentModifierMask(modifiers);
        }
        if let Some(action) = action {
            // SAFETY: the selector is a valid action for the target; with
            // a nil target it is dispatched through the responder chain.
            unsafe { item.setAction(Some(action)) };
        }
        if let Some(target) = target {
            // SAFETY: the target (the window delegate) outlives the menu.
            unsafe { item.setTarget(Some(target)) };
        }
        item
    }

    /// A top-level menu item titled `title` whose submenu contains `items`.
    fn make_menu_with_items(
        mtm: MainThreadMarker,
        title: &str,
        items: &[Retained<NSMenuItem>],
    ) -> Retained<NSMenuItem> {
        let menu_item = NSMenuItem::new(mtm);
        menu_item.setTitle(&NSString::from_str(title));
        let menu = NSMenu::new(mtm);
        for item in items {
            menu.addItem(item);
        }
        menu_item.setSubmenu(Some(&menu));
        menu_item
    }

    // ── Event monitor ──────────────────────────────────────────────────────

    fn install_event_monitor(&mut self) -> Result<(), String> {
        let handle = self.handle.expect("app handle must be installed");
        let block: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent> =
            RcBlock::new(move |event: NonNull<NSEvent>| {
                let app = unsafe { &mut *handle.app_ptr() };
                app.handle_ns_event(event)
            });
        // SAFETY: the block is valid and lives for as long as the monitor.
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::Any, &block)
        };
        let Some(monitor) = monitor else {
            return Err(String::from("failed to install the local event monitor"));
        };
        self.event_monitor = Some(monitor);
        Ok(())
    }

    /// Route a raw NSEvent. Keyboard events go to the app unless the native
    /// address field is editing; mouse and scroll events inside the web
    /// content area are routed to the content. Everything else is passed
    /// through to AppKit (the native chrome controls, the titlebar, other
    /// windows).
    fn handle_ns_event(&mut self, event: NonNull<NSEvent>) -> *mut NSEvent {
        let event_ref = unsafe { event.as_ref() };
        let event_type = event_ref.r#type();
        match event_type {
            NSEventType::KeyDown | NSEventType::KeyUp => {
                if self.address_field_is_first_responder() {
                    // The address field is editing: let AppKit deliver the
                    // event to the field (IME, shortcuts, etc.).
                    event.as_ptr()
                } else if event_type == NSEventType::KeyDown
                    && event_ref
                        .modifierFlags()
                        .contains(NSEventModifierFlags::Command)
                    && self.menu_performs_key_equivalent(event_ref)
                {
                    // The main menu matched a ⌘-shortcut (⌘T, ⌘W, ⌘L, …)
                    // and has dispatched the action: consume the event so
                    // it does not also reach the web content.
                    std::ptr::null_mut()
                } else {
                    self.handle_content_keyboard_event(event_ref);
                    std::ptr::null_mut()
                }
            }
            NSEventType::FlagsChanged => event.as_ptr(),
            NSEventType::LeftMouseDown
            | NSEventType::LeftMouseUp
            | NSEventType::LeftMouseDragged
            | NSEventType::RightMouseDown
            | NSEventType::RightMouseUp
            | NSEventType::RightMouseDragged
            | NSEventType::OtherMouseDown
            | NSEventType::OtherMouseUp
            | NSEventType::OtherMouseDragged
            | NSEventType::MouseMoved
            | NSEventType::ScrollWheel => {
                let Some(window_id) = self.window_id_for_ns_event(event_ref) else {
                    return event.as_ptr();
                };
                // Keep the tab hover state synced to the cursor on every
                // pointer event, so it cannot go stale.
                self.update_tab_hover_from_event(window_id, event_ref);
                // A mouse-down on the address field focuses it: draw the
                // focus border immediately and select the URL once the
                // click has been delivered (see `address_field_clicked`).
                // This does not depend on the field-editor notifications,
                // whose timing varies.
                if matches!(
                    event_type,
                    NSEventType::LeftMouseDown
                        | NSEventType::RightMouseDown
                        | NSEventType::OtherMouseDown
                ) && self.click_is_on_address_field(window_id, event_ref)
                {
                    self.address_field_clicked(window_id);
                    return event.as_ptr();
                }
                // While AppKit's live-resize tracking loop is running
                // (the user is dragging a resize handle), every mouse
                // event belongs to that loop: consuming any of them
                // stalls the drag and leaves the resize cursor stuck.
                if self.window_in_live_resize(window_id) {
                    return event.as_ptr();
                }
                let Some((x, y_from_top)) = self.content_point(window_id, event_ref) else {
                    return event.as_ptr();
                };
                let (width, height) = self
                    .windows
                    .get(&window_id)
                    .map(|window_state| window_state.content_size)
                    .unwrap_or_default();
                // The initial mouse-down in the window frame's resize
                // region (bottom edge, side edges, bottom corners) must
                // reach AppKit so the resize tracking loop starts.
                if Self::in_window_resize_region(x, y_from_top, width, height) {
                    return event.as_ptr();
                }
                self.handle_content_mouse_event(window_id, event_ref, event_type, x, y_from_top);
                std::ptr::null_mut()
            }
            _ => event.as_ptr(),
        }
    }

    /// True while the window is in AppKit's live-resize tracking loop (the
    /// user is dragging a resize handle).
    fn window_in_live_resize(&self, window_id: WindowId) -> bool {
        self.windows
            .get(&window_id)
            .is_some_and(|window_state| window_state.window.inLiveResize())
    }

    /// Whether a point in the content view's coordinate space (x right,
    /// y from top) lies in the window frame's resize region: the bottom
    /// edge, the left/right edges, or the bottom corners (the grow box).
    /// Events there belong to AppKit's resize tracking, not the content.
    fn in_window_resize_region(x: f64, y_from_top: f64, width: f64, height: f64) -> bool {
        let on_edge = x <= WINDOW_RESIZE_MARGIN
            || x >= width - WINDOW_RESIZE_MARGIN
            || y_from_top >= height - WINDOW_RESIZE_MARGIN;
        let on_bottom_corner = y_from_top >= height - WINDOW_RESIZE_CORNER_RADIUS
            && (x <= WINDOW_RESIZE_CORNER_RADIUS || x >= width - WINDOW_RESIZE_CORNER_RADIUS);
        on_edge || on_bottom_corner
    }

    /// True while the native address field is being edited: the window's
    /// first responder is the field's field editor (an internal NSTextView),
    /// not the NSTextField itself.
    fn address_field_is_editing(window_state: &MacWindow) -> bool {
        let Some(editor) = window_state.address_field.currentEditor() else {
            return false;
        };
        window_state
            .window
            .firstResponder()
            .is_some_and(|responder| {
                // SAFETY: `isEqual` is identity comparison for Objective-C
                // objects.
                let same: bool = unsafe { msg_send![&responder, isEqual: &*editor] };
                same
            })
    }

    fn address_field_is_first_responder(&self) -> bool {
        self.active_window_id.is_some_and(|window_id| {
            self.windows
                .get(&window_id)
                .is_some_and(Self::address_field_is_editing)
        })
    }

    fn window_id_for_ns_event(&self, event: &NSEvent) -> Option<WindowId> {
        let event_window = event.window(self.mtm)?;
        self.windows.iter().find_map(|(window_id, window_state)| {
            if std::ptr::eq(&*event_window, &*window_state.window) {
                Some(*window_id)
            } else {
                None
            }
        })
    }

    /// The event location in the content view's top-left coordinate space
    /// (points). Returns `None` when the event is not inside the content
    /// view (e.g. the titlebar), which is left to AppKit.
    fn content_point(&self, window_id: WindowId, event: &NSEvent) -> Option<(f64, f64)> {
        let window_state = self.windows.get(&window_id)?;
        let (width, height) = window_state.content_size;
        let location = event.locationInWindow();
        let x = location.x;
        let y_from_top = height - location.y;
        if x < 0.0 || y_from_top < 0.0 || x > width || y_from_top > height {
            None
        } else {
            Some((x, y_from_top))
        }
    }

    /// Ask the main menu to perform the key equivalent for this event.
    /// The local event monitor sees key events before the menu system
    /// does, so ⌘-shortcuts bound to menu items are executed here and
    /// consumed; unbound ⌘-combinations return false and keep flowing to
    /// the web content (pages keep their own ⌘-shortcuts).
    fn menu_performs_key_equivalent(&self, event: &NSEvent) -> bool {
        let Some(menu) = self.ns_app.mainMenu() else {
            return false;
        };
        menu.performKeyEquivalent(event)
    }

    fn handle_content_keyboard_event(&mut self, event: &NSEvent) {
        let Some(window_id) = self.active_window_id else {
            return;
        };

        let key = input::ns_event_to_key_event(event);
        let ui_event = if event.r#type() == NSEventType::KeyDown {
            UiEvent::KeyDown(key)
        } else {
            UiEvent::KeyUp(key)
        };
        self.dispatch_to_content(window_id, ui_event);
    }

    fn handle_content_mouse_event(
        &mut self,
        window_id: WindowId,
        event: &NSEvent,
        event_type: NSEventType,
        x: f64,
        y_from_top: f64,
    ) {
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        window_state.keyboard_modifiers = input::modifiers_from_flags(event.modifierFlags());
        let modifiers = window_state.keyboard_modifiers;

        let is_down = matches!(
            event_type,
            NSEventType::LeftMouseDown | NSEventType::RightMouseDown | NSEventType::OtherMouseDown
        );
        let is_up = matches!(
            event_type,
            NSEventType::LeftMouseUp | NSEventType::RightMouseUp | NSEventType::OtherMouseUp
        );
        if is_down || is_up {
            let button = input::button_from_number(event.buttonNumber());
            if is_down {
                window_state.buttons |= button.into();
            } else {
                window_state.buttons.remove(button.into());
            }
        }
        let buttons = window_state.buttons;

        if is_down {
            // Clicking the content hands text-input focus away from the
            // native address field, so the next keystrokes go to the page,
            // and clears its focus border.
            window_state.window.makeFirstResponder(None);
            window_state.address_field_focused = false;
            Self::set_address_field_focus_style(&window_state.address_field, false);
        }
        let coords = input::content_coords(x, y_from_top);

        let event_kind = if is_down {
            UiEventKind::PointerDown
        } else if is_up {
            UiEventKind::PointerUp
        } else if event_type == NSEventType::ScrollWheel {
            UiEventKind::Wheel
        } else {
            UiEventKind::PointerMove
        };

        let ui_event = |kind: UiEventKind, pointer: BlitzPointerEvent| match kind {
            UiEventKind::PointerDown => UiEvent::PointerDown(pointer),
            UiEventKind::PointerUp => UiEvent::PointerUp(pointer),
            UiEventKind::PointerMove => UiEvent::PointerMove(pointer),
            UiEventKind::Wheel => UiEvent::Wheel(BlitzWheelEvent {
                delta: input::wheel_delta_from_event(event),
                coords: pointer.coords,
                buttons: pointer.buttons,
                mods: pointer.mods,
            }),
        };

        let pointer = input::pointer_event(
            BlitzPointerId::Mouse,
            true,
            coords,
            input::button_from_number(event.buttonNumber()),
            buttons,
            modifiers,
        );
        self.dispatch_to_content(window_id, ui_event(event_kind, pointer));
    }

    // ── Native chrome ──────────────────────────────────────────────────────

    fn make_tab_button(
        mtm: MainThreadMarker,
        delegate: &Delegate,
        label: &str,
        index: usize,
        active: bool,
    ) -> Retained<NSButton> {
        let button = NSButton::new(mtm);
        button.setTitle(&NSString::from_str(label));
        button.setButtonType(NSButtonType::MomentaryPushIn);
        // Borderless: the tab cell's pill (see `make_tab_cell`) draws the
        // selected state; the button itself is just the label.
        button.setBordered(false);
        button.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        button.setTag(index as NSInteger);
        // Long titles truncate at the trailing edge when tabs get narrow.
        button.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        // The tint carries the tab state: the active tab reads in the
        // primary label color, inactive tabs in the secondary (Safari
        // greys the inactive ones out).
        let tint = if active {
            NSColor::labelColor()
        } else {
            NSColor::secondaryLabelColor()
        };
        button.setContentTintColor(Some(&tint));
        // No focus ring: the tab cell's pill indicates selection, and the
        // ring would read as a text-field affordance.
        button.setFocusRingType(objc2_app_kit::NSFocusRingType::None);
        // SAFETY: the delegate is a valid target and the selector matches
        // its `switchTab:` action.
        let _: () = unsafe { msg_send![&button, setTarget: delegate] };
        unsafe { button.setAction(Some(sel!(switchTab:))) };
        button
    }

    fn make_tab_cell(
        mtm: MainThreadMarker,
        active: bool,
        label_button: &NSButton,
        close_button: &NSButton,
    ) -> Retained<NSView> {
        let cell = NSView::new(mtm);
        cell.setWantsLayer(true);
        cell.addSubview(label_button);
        cell.addSubview(close_button);
        if let Some(layer) = cell.layer() {
            layer.setCornerRadius(TAB_CORNER_RADIUS);
            Self::set_tab_cell_pill(
                &cell,
                if active {
                    TabPill::Active
                } else {
                    TabPill::None
                },
            );
        }
        cell
    }

    fn make_tab_close_button(
        mtm: MainThreadMarker,
        delegate: &Delegate,
        index: usize,
    ) -> Retained<NSButton> {
        let button = NSButton::new(mtm);
        // A template SF Symbol ✕ renders with the current label color and
        // adapts to light/dark; the literal "✕" glyph it replaces did
        // not.
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("xmark"),
            None,
        );
        button.setImage(image.as_deref());
        button.setImagePosition(NSCellImagePosition::ImageOnly);
        button.setButtonType(NSButtonType::MomentaryChange);
        // Borderless: the cell's pill is the tab's background; the close
        // button is just the glyph.
        button.setBordered(false);
        button.setTag(index as NSInteger);
        // SAFETY: the delegate is a valid target and the selector matches
        // its `closeTabAt:` action.
        let _: () = unsafe { msg_send![&button, setTarget: delegate] };
        unsafe { button.setAction(Some(sel!(closeTabAt:))) };
        button
    }

    fn make_address_field(mtm: MainThreadMarker, delegate: &Delegate) -> Retained<NSTextField> {
        let field = NSTextField::new(mtm);
        field.setEditable(true);
        field.setSelectable(true);
        field.setUsesSingleLineMode(true);
        field.setDrawsBackground(true);
        field.setBezelStyle(NSTextFieldBezelStyle::RoundedBezel);
        field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        field.setPlaceholderString(Some(&NSString::from_str("Search or enter address")));
        // The system focus ring's shape does not match the tight accent
        // border Safari draws around the field; a dedicated sublayer draws
        // that border instead, toggled on focus (see
        // `set_address_field_focus_style`). The border lives on a sublayer
        // rather than the field's own layer because AppKit manages the
        // latter for controls and may reset it when the field redraws.
        field.setFocusRingType(objc2_app_kit::NSFocusRingType::None);
        field.setWantsLayer(true);
        if let Some(layer) = field.layer() {
            let border_layer = CALayer::layer();
            border_layer.setName(Some(&NSString::from_str("address-focus-border")));
            border_layer.setFrame(layer.bounds());
            border_layer.setAutoresizingMask(
                CAAutoresizingMask::LayerWidthSizable | CAAutoresizingMask::LayerHeightSizable,
            );
            border_layer.setCornerRadius(6.0);
            border_layer.setBorderWidth(0.0);
            layer.addSublayer(&border_layer);
        }
        // The delegate drives the focus border and URL selection via
        // `controlTextDidBeginEditing:`/`controlTextDidEndEditing:`.
        // SAFETY: the delegate implements `NSTextFieldDelegate` and
        // outlives the field (it is retained by the app).
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(delegate))) };
        // SAFETY: the delegate is a valid target and the selector matches
        // its `navigate:` action; the action fires on Return.
        let _: () = unsafe { msg_send![&field, setTarget: delegate] };
        unsafe { field.setAction(Some(sel!(navigate:))) };
        field
    }

    // ── Toolbar ───────────────────────────────────────────────────────────

    /// The window owning `toolbar` (the toolbar delegate is shared by all
    /// windows; each toolbar's items carry that window's chrome views).
    fn window_for_toolbar(&self, toolbar: &NSToolbar) -> Option<WindowId> {
        self.windows.iter().find_map(|(window_id, window_state)| {
            if std::ptr::eq(&*window_state.toolbar, toolbar) {
                Some(*window_id)
            } else {
                None
            }
        })
    }

    /// Build the toolbar item for an identifier, as asked by the toolbar
    /// delegate. Custom-view items (tabs, address) return the window's
    /// views; when the request is for the customization palette (not an
    /// insertion) they return a labeled placeholder so the real views stay
    /// in the toolbar.
    fn make_toolbar_item(
        &mut self,
        toolbar: &NSToolbar,
        identifier: &NSToolbarItemIdentifier,
        will_be_inserted: bool,
    ) -> Option<Retained<NSToolbarItem>> {
        let identifier = identifier.to_string();
        match identifier.as_str() {
            "backForward" => Some(Self::make_back_forward_group(self.mtm)),
            "newTab" => Some(Self::make_nav_toolbar_item(
                self.mtm,
                &identifier,
                "New Tab",
                "plus",
                Some(sel!(newTab:)),
                Some(&*self.delegate),
                true,
            )),
            "reload" => Some(Self::make_nav_toolbar_item(
                self.mtm,
                &identifier,
                "Reload",
                "arrow.clockwise",
                Some(sel!(reload:)),
                Some(&*self.delegate),
                true,
            )),
            "tabs" | "address" if !will_be_inserted => {
                // The customization palette requests a representation
                // without inserting the item.
                Some(Self::make_nav_toolbar_item(
                    self.mtm,
                    &identifier,
                    &identifier,
                    "",
                    None,
                    None,
                    true,
                ))
            }
            "address" => {
                let window_id = self.window_for_toolbar(toolbar)?;
                let window_state = self.windows.get(&window_id)?;
                Some(Self::make_custom_view_toolbar_item(
                    self.mtm,
                    "address",
                    &window_state.address_field,
                    NSSize::new(TOOLBAR_ITEM_MIN_WIDTH, ADDRESS_FIELD_HEIGHT),
                    NSSize::new(ADDRESS_FIELD_MAX_WIDTH, ADDRESS_FIELD_HEIGHT),
                    "Address",
                ))
            }
            _ => None,
        }
    }

    /// A toolbar item with an image (SF Symbol), label, action, and target.
    fn make_nav_toolbar_item(
        mtm: MainThreadMarker,
        identifier: &str,
        label: &str,
        symbol: &str,
        action: Option<Sel>,
        target: Option<&AnyObject>,
        enabled: bool,
    ) -> Retained<NSToolbarItem> {
        // The item is initialized with the standard initializer.
        let item = NSToolbarItem::initWithItemIdentifier(
            NSToolbarItem::alloc(mtm),
            &NSString::from_str(identifier),
        );
        item.setLabel(&NSString::from_str(label));
        item.setPaletteLabel(&NSString::from_str(label));
        if !symbol.is_empty() {
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(symbol),
                None,
            );
            if let Some(image) = image {
                // Toolbar icons read better at a slightly heavier weight
                // than the SF Symbol default; keep the system point size
                // and scale.
                let configuration =
                    NSImageSymbolConfiguration::configurationWithPointSize_weight_scale(
                        0.0,
                        unsafe { NSFontWeightMedium },
                        NSImageSymbolScale::Medium,
                    );
                if let Some(configured) = image.imageWithSymbolConfiguration(&configuration) {
                    item.setImage(Some(&configured));
                }
            }
        }
        if let Some(action) = action {
            // SAFETY: the selector is valid for the target; with a nil
            // target it is dispatched through the responder chain.
            unsafe { item.setAction(Some(action)) };
        }
        if let Some(target) = target {
            // SAFETY: the target (the window delegate) outlives the
            // toolbar.
            unsafe { item.setTarget(Some(target)) };
        }
        item.setEnabled(enabled);
        item
    }

    /// The Back/Forward pair as a single joined control (an
    /// `NSToolbarItemGroup` in expanded representation), the Safari-style
    /// look, instead of two loose toolbar items.
    fn make_back_forward_group(mtm: MainThreadMarker) -> Retained<NSToolbarItem> {
        let group = NSToolbarItemGroup::initWithItemIdentifier(
            NSToolbarItemGroup::alloc(mtm),
            &NSString::from_str("backForward"),
        );
        let back = Self::make_nav_toolbar_item(
            mtm,
            "back",
            "Back",
            "chevron.left",
            Some(sel!(goBack:)),
            None,
            false,
        );
        let forward = Self::make_nav_toolbar_item(
            mtm,
            "forward",
            "Forward",
            "chevron.right",
            Some(sel!(goForward:)),
            None,
            false,
        );
        group.setSubitems(&NSArray::<NSToolbarItem>::from_slice(&[&back, &forward]));
        group.setControlRepresentation(NSToolbarItemGroupControlRepresentation::Expanded);
        // Session history is not implemented yet, so the pair stays
        // disabled (matching the History menu's Back/Forward items).
        group.setEnabled(false);
        group.into_super()
    }

    /// A toolbar item hosting a custom view (the address field), flexible
    /// within the given min/max sizes.
    fn make_custom_view_toolbar_item(
        mtm: MainThreadMarker,
        identifier: &str,
        view: &NSView,
        min_size: NSSize,
        max_size: NSSize,
        label: &str,
    ) -> Retained<NSToolbarItem> {
        // The item is initialized with the standard initializer.
        let item = NSToolbarItem::initWithItemIdentifier(
            NSToolbarItem::alloc(mtm),
            &NSString::from_str(identifier),
        );
        item.setLabel(&NSString::from_str(label));
        item.setPaletteLabel(&NSString::from_str(label));
        item.setView(Some(view));
        // The min/max sizes make the item flexible within the toolbar row.
        // They are deprecated in favor of Auto Layout constraints, but a
        // plain container view has no intrinsic size to measure, and the
        // constraint-based equivalent cannot express "grow with the toolbar"
        // as simply; the deprecated properties still function.
        #[allow(deprecated)]
        item.setMinSize(min_size);
        #[allow(deprecated)]
        item.setMaxSize(max_size);
        item
    }

    fn refresh_tab_strip(&mut self, window_id: WindowId) {
        let delegate = self.delegate.clone();
        let mtm = self.mtm;
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        for cell in window_state.tab_cells.drain(..) {
            cell.removeFromSuperview();
        }
        for button in window_state.tab_buttons.drain(..) {
            button.removeFromSuperview();
        }
        for button in window_state.tab_close_buttons.drain(..) {
            button.removeFromSuperview();
        }
        for (index, webview_id) in window_state.tab_order.iter().enumerate() {
            let label = Self::tab_label(window_state, webview_id);
            let active = window_state.active_tab == Some(*webview_id);
            let label_button = Self::make_tab_button(mtm, &delegate, &label, index, active);
            window_state.tab_buttons.push(label_button.clone());
            let close_button = Self::make_tab_close_button(mtm, &delegate, index);
            window_state.tab_close_buttons.push(close_button.clone());
            let cell = Self::make_tab_cell(mtm, active, &label_button, &close_button);
            window_state.tab_cells.push(cell.clone());
            window_state.tab_strip.addSubview(&cell);
        }
        Self::layout_tab_strip(window_state);
        // Re-apply the hover state after a rebuild (e.g. a page title
        // arriving while the pointer is over a tab); the next pointer
        // event re-syncs it from the cursor position.
        if let Some(hovered) = window_state.hovered_tab
            && let Some(cell) = window_state.tab_cells.get(hovered)
        {
            Self::set_tab_cell_pill(cell, TabPill::Hover);
        }
    }

    fn refresh_address_field(&mut self, window_id: WindowId) {
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let address = window_state
            .active_tab
            .and_then(|webview_id| window_state.tabs.get(&webview_id))
            .map(TabState::display_url)
            .unwrap_or_default();
        // Don't clobber the field while the user is typing into it.
        if !Self::address_field_is_editing(window_state) {
            window_state
                .address_field
                .setStringValue(&NSString::from_str(&address));
        }
    }

    /// The address field gained focus: draw the focus border and, on the
    /// first focus of each editing session, select the whole URL.
    fn address_field_begin_editing(&mut self) {
        let Some(window_id) = self.active_window_id else {
            return;
        };
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        // Select-all only on the first focus of an editing session; a
        // subsequent click while already editing places the cursor
        // instead (Safari behavior).
        if !window_state.address_field_focused {
            window_state.address_field_focused = true;
            Self::select_all_address_field(&window_state.address_field);
        }
        Self::set_address_field_focus_style(&window_state.address_field, true);
    }

    /// The address field lost focus: clear the focus border.
    fn address_field_end_editing(&mut self) {
        let Some(window_id) = self.active_window_id else {
            return;
        };
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        window_state.address_field_focused = false;
        Self::set_address_field_focus_style(&window_state.address_field, false);
    }

    /// The Safari-style focus indication: a tight accent-colored border
    /// on the field's layer, drawn instead of the system focus ring.
    fn set_address_field_focus_style(field: &NSTextField, focused: bool) {
        let Some(layer) = field.layer() else {
            return;
        };
        let Some(border_layer) = Self::address_field_border_layer(&layer) else {
            return;
        };
        // Keep the border covering the field; the autoresizing mask usually
        // handles resizes, but re-syncing here is cheap insurance.
        border_layer.setFrame(layer.bounds());
        if focused {
            border_layer.setBorderWidth(2.0);
            border_layer.setBorderColor(Some(&NSColor::controlAccentColor().CGColor()));
        } else {
            border_layer.setBorderWidth(0.0);
        }
    }

    /// Whether `event`'s location lies within the address field's frame
    /// (in window coordinates).
    fn click_is_on_address_field(&self, window_id: WindowId, event: &NSEvent) -> bool {
        let Some(window_state) = self.windows.get(&window_id) else {
            return false;
        };
        let location = event.locationInWindow();
        let field_frame = window_state.address_field.convertRect_toView(
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                window_state.address_field.bounds().size,
            ),
            None,
        );
        location.x >= field_frame.origin.x
            && location.x <= field_frame.origin.x + field_frame.size.width
            && location.y >= field_frame.origin.y
            && location.y <= field_frame.origin.y + field_frame.size.height
    }

    /// A mouse-down landed on the address field: draw the focus border
    /// and, on the first click of an editing session, select the whole
    /// URL once the click has been delivered (so the click does not
    /// collapse the selection).
    fn address_field_clicked(&mut self, window_id: WindowId) {
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        Self::set_address_field_focus_style(&window_state.address_field, true);
        if !window_state.address_field_focused {
            window_state.address_field_focused = true;
            let handle = self.handle.expect("app handle must be installed");
            dispatch::Queue::main().exec_async(move || {
                let app = unsafe { &mut *handle.app_ptr() };
                app.deferred_address_field_select(window_id);
            });
        }
    }

    /// The deferred half of `address_field_clicked`: select the field's
    /// contents if it is still the one being edited.
    fn deferred_address_field_select(&mut self, window_id: WindowId) {
        let Some(window_state) = self.windows.get(&window_id) else {
            return;
        };
        if !Self::address_field_is_editing(window_state) {
            return;
        }
        Self::select_all_address_field(&window_state.address_field);
    }

    /// Select the field's whole contents via its field editor, without
    /// restarting the editing session (`selectText:` would re-enter the
    /// first-responder machinery, ending and restarting the session).
    fn select_all_address_field(field: &NSTextField) {
        let Some(editor) = field.currentEditor() else {
            return;
        };
        let length = field.stringValue().len_utf16();
        editor.setSelectedRange(NSRange::new(0, length));
    }

    /// The field's focus-border sublayer (created in `make_address_field`),
    /// found by name.
    fn address_field_border_layer(layer: &CALayer) -> Option<Retained<CALayer>> {
        // SAFETY: the sublayer array belongs to the field's layer.
        let sublayers = unsafe { layer.sublayers() }?;
        sublayers.iter().find(|sublayer| {
            sublayer
                .name()
                .is_some_and(|name| name.to_string() == "address-focus-border")
        })
    }

    fn refresh_chrome(&mut self, window_id: WindowId) {
        self.refresh_tab_strip(window_id);
        self.refresh_address_field(window_id);
        // Mirror the active tab's label in the window title until page
        // titles are plumbed from the content process.
        let Some(window_state) = self.windows.get(&window_id) else {
            return;
        };
        let title = window_state
            .active_tab
            .and_then(|webview_id| window_state.tabs.get(&webview_id))
            .map(Self::tab_label_for_tab)
            .unwrap_or_else(|| String::from("formal-web"));
        window_state.window.setTitle(&NSString::from_str(&title));
    }

    fn tab_label(window_state: &MacWindow, webview_id: &WebviewId) -> String {
        match window_state.tabs.get(webview_id) {
            Some(tab) => Self::tab_label_for_tab(tab),
            None => String::from("New Tab"),
        }
    }

    fn tab_label_for_tab(tab: &TabState) -> String {
        // The tab label is the page title when available, falling back to
        // the URL for documents without one (and "New Tab" for blank
        // pages).
        if let Some(title) = &tab.page_title
            && !title.is_empty()
        {
            return title.clone();
        }
        if let Some(url) = &tab.committed_url
            && !url.is_empty()
        {
            return Self::truncate_url(url);
        }
        if let Some(url) = &tab.pending_url
            && !url.is_empty()
        {
            return Self::truncate_url(url);
        }
        String::from("New Tab")
    }

    fn truncate_url(url: &str) -> String {
        let display = if let Some(path) = url.strip_prefix("file://") {
            // For local files the last path component is the useful label
            // (a directory or the file name); the full path duplicates the
            // address field and is unreadable once narrowed.
            let trimmed = path.trim_end_matches('/');
            trimmed
                .rsplit('/')
                .find(|component| !component.is_empty())
                .unwrap_or(trimmed)
        } else {
            url.strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .unwrap_or(url)
        };
        if display.chars().count() > 24 {
            let truncated: String = display.chars().take(21).collect();
            format!("{truncated}…")
        } else {
            display.to_owned()
        }
    }

    /// Lay out the chrome rows: the tab strip spans the content area just
    /// below the titlebar and toolbar, with the web content filling the
    /// rest.
    fn layout_window_views(window_state: &mut MacWindow) {
        let rect = window_state.window.contentLayoutRect();
        let web_height = (rect.size.height - TAB_STRIP_HEIGHT).max(0.0);
        window_state.content_size = (rect.size.width, web_height);
        // The content view is not flipped: frames use a bottom-left origin,
        // so the web content sits at the bottom and the tab strip above it.
        window_state.web_view.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(rect.size.width, web_height),
        ));
        // Keep the layer-hosting layer's frame in sync with the view.
        window_state.web_layer.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(rect.size.width, web_height),
        ));
        window_state.tab_strip.setFrame(NSRect::new(
            NSPoint::new(0.0, web_height),
            NSSize::new(rect.size.width, TAB_STRIP_HEIGHT),
        ));
        Self::layout_tab_strip(window_state);
    }

    fn layout_tab_strip(window_state: &mut MacWindow) {
        let strip_width = window_state.tab_strip.frame().size.width;
        let count = window_state.tab_cells.len();
        // Tabs start at the leading edge at the max width and shrink as
        // more open (Safari-style) instead of overflowing at a fixed
        // width.
        let edge = 8.0;
        let gaps = TAB_CELL_GAP * (count.saturating_sub(1) as f64);
        let available = (strip_width - 2.0 * edge - gaps).max(0.0);
        let cell_width =
            (available / (count.max(1) as f64)).clamp(MIN_TAB_CELL_WIDTH, TAB_BUTTON_WIDTH);
        let button_y = ((TAB_STRIP_HEIGHT - TAB_BUTTON_HEIGHT) / 2.0).max(0.0);
        for (index, cell) in window_state.tab_cells.iter().enumerate() {
            let x = edge + (index as f64) * (cell_width + TAB_CELL_GAP);
            cell.setFrame(NSRect::new(
                NSPoint::new(x, button_y),
                NSSize::new(cell_width, TAB_BUTTON_HEIGHT),
            ));
            // Inside the cell: the label fills the space before the close
            // button, which sits at the trailing edge.
            let close_x = (cell_width - TAB_CLOSE_BUTTON_WIDTH).max(0.0);
            if let Some(label_button) = window_state.tab_buttons.get(index) {
                label_button.setFrame(NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(close_x, TAB_BUTTON_HEIGHT),
                ));
            }
            if let Some(close_button) = window_state.tab_close_buttons.get(index) {
                close_button.setFrame(NSRect::new(
                    NSPoint::new(close_x, 0.0),
                    NSSize::new(TAB_CLOSE_BUTTON_WIDTH, TAB_BUTTON_HEIGHT),
                ));
            }
        }
    }

    // ── Chrome actions ─────────────────────────────────────────────────────

    fn tab_cell_is_active(window_state: &MacWindow, index: usize) -> bool {
        window_state
            .tab_order
            .get(index)
            .is_some_and(|webview_id| window_state.active_tab == Some(*webview_id))
    }

    /// Update the tab hover state from the pointer's current location.
    /// Driven by the event monitor on every pointer event, so the hover
    /// always matches the cursor (tracking-area exit events were missed
    /// when the strip was rebuilt under the cursor).
    fn update_tab_hover_from_event(&mut self, window_id: WindowId, event: &NSEvent) {
        let location = event.locationInWindow();
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let hovered = window_state
            .tab_cells
            .iter()
            .enumerate()
            .find_map(|(index, cell)| {
                let frame = cell.convertRect_toView(
                    NSRect::new(NSPoint::new(0.0, 0.0), cell.bounds().size),
                    None,
                );
                (location.x >= frame.origin.x
                    && location.x <= frame.origin.x + frame.size.width
                    && location.y >= frame.origin.y
                    && location.y <= frame.origin.y + frame.size.height)
                    .then_some(index)
            });
        if window_state.hovered_tab == hovered {
            return;
        }
        let previous = window_state.hovered_tab;
        window_state.hovered_tab = hovered;
        // Restore the previously hovered cell's resting pill.
        if let Some(previous) = previous
            && let Some(cell) = window_state.tab_cells.get(previous)
        {
            Self::set_tab_cell_pill(
                cell,
                if Self::tab_cell_is_active(window_state, previous) {
                    TabPill::Active
                } else {
                    TabPill::None
                },
            );
        }
        // Apply the hover pill to the cell under the cursor.
        if let Some(hovered) = hovered
            && let Some(cell) = window_state.tab_cells.get(hovered)
        {
            Self::set_tab_cell_pill(cell, TabPill::Hover);
        }
    }

    /// Clear the tab hover state: restore the hovered cell's resting pill
    /// and forget the hovered index. Called when the window stops being
    /// interactive (resigns key, miniaturizes).
    fn clear_tab_hover(&mut self, window_id: WindowId) {
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let Some(hovered) = window_state.hovered_tab.take() else {
            return;
        };
        if let Some(cell) = window_state.tab_cells.get(hovered) {
            Self::set_tab_cell_pill(
                cell,
                if Self::tab_cell_is_active(window_state, hovered) {
                    TabPill::Active
                } else {
                    TabPill::None
                },
            );
        }
    }

    /// Set the tab pill's background fill on a cell's layer.
    fn set_tab_cell_pill(cell: &NSView, state: TabPill) {
        let Some(layer) = cell.layer() else {
            return;
        };
        let background = match state {
            TabPill::None => None,
            // The active tab reads as a light grey pill, lighter than the
            // hover fill, so the hover effect stays visible on it.
            TabPill::Active => Some(NSColor::secondaryLabelColor().colorWithAlphaComponent(0.10)),
            TabPill::Hover => Some(NSColor::secondaryLabelColor().colorWithAlphaComponent(0.16)),
        };
        let Some(background) = background else {
            layer.setBackgroundColor(None);
            return;
        };
        layer.setBackgroundColor(Some(&background.CGColor()));
    }

    fn action_switch_tab(&mut self, index: usize) {
        let Some(window_id) = self.active_window_id else {
            return;
        };
        let webview_id = self
            .windows
            .get(&window_id)
            .and_then(|window_state| window_state.tab_order.get(index).copied());
        let Some(webview_id) = webview_id else {
            return;
        };
        if let Some(window_state) = self.windows.get_mut(&window_id) {
            window_state.active_tab = Some(webview_id);
            Self::present_active_surface(window_state);
        }
        self.refresh_chrome(window_id);
        self.update_provider_viewport(window_id);
        if let Some(provider) = self.provider.as_ref() {
            let _ = provider.frame_needed(webview_id);
        }
    }

    fn action_new_tab(&mut self) {
        // New tabs start on a blank page; the startup destination is only
        // loaded into the first window at launch.
        if let Some(provider) = self.provider.as_ref() {
            let _ = provider.navigate(None, "about:blank");
        }
    }

    fn action_new_window(&mut self) {
        // New windows start on a blank page; the startup destination is
        // only loaded into the first window at launch.
        let _ = self.create_window("formal-web", "about:blank");
    }

    fn action_close_tab(&mut self) {
        let Some(window_id) = self.active_window_id else {
            return;
        };
        let Some(webview_id) = self
            .windows
            .get(&window_id)
            .and_then(|window_state| window_state.active_tab)
        else {
            return;
        };
        self.close_tab(window_id, webview_id);
    }

    fn action_close_tab_at(&mut self, index: usize) {
        let Some(window_id) = self.active_window_id else {
            return;
        };
        let webview_id = self
            .windows
            .get(&window_id)
            .and_then(|window_state| window_state.tab_order.get(index).copied());
        let Some(webview_id) = webview_id else {
            return;
        };
        self.close_tab(window_id, webview_id);
    }

    fn action_close_window(&mut self) {
        let Some(window_id) = self.active_window_id else {
            return;
        };
        self.close_window(window_id);
    }

    fn action_reload(&mut self) {
        let Some(window_id) = self.active_window_id else {
            return;
        };
        let Some(webview_id) = self
            .windows
            .get(&window_id)
            .and_then(|window_state| window_state.active_tab)
        else {
            return;
        };
        let Some(url) = self
            .windows
            .get(&window_id)
            .and_then(|window_state| window_state.tabs.get(&webview_id))
            .and_then(|tab| tab.committed_url.clone())
        else {
            return;
        };
        // Session history (and hence a dedicated reload command) is not
        // implemented in the user agent yet; re-navigating to the committed
        // URL performs a fresh load.
        if let Some(provider) = self.provider.as_ref() {
            let _ = provider.navigate(Some(webview_id), &url);
        }
        if let Some(window_state) = self.windows.get_mut(&window_id)
            && let Some(tab) = window_state.tabs.get_mut(&webview_id)
        {
            tab.pending_url = Some(url);
        }
        self.refresh_address_field(window_id);
    }

    fn action_focus_address(&mut self) {
        let Some(window_id) = self.active_window_id else {
            return;
        };
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        if !Self::address_field_is_editing(window_state) {
            // Focusing the field begins an editing session; the delegate's
            // begin-editing handler selects the URL and draws the focus
            // border.
            window_state
                .window
                .makeFirstResponder(Some(&window_state.address_field));
        } else {
            // Already editing: select the URL directly without restarting
            // the editing session.
            Self::select_all_address_field(&window_state.address_field);
        }
        window_state.address_field_focused = true;
        Self::set_address_field_focus_style(&window_state.address_field, true);
    }

    /// Close a tab: drop its window state, surfaces, and chrome buttons,
    /// then show the rightmost remaining tab. Closing the last tab closes
    /// the window (which exits the app when it was the only window). The
    /// underlying traversable in the user agent is not destroyed — the
    /// user agent has no webview teardown path yet, the same situation as
    /// closing a window.
    fn close_tab(&mut self, window_id: WindowId, webview_id: WebviewId) {
        let (empty, closed_active, window) = {
            let Some(window_state) = self.windows.get_mut(&window_id) else {
                return;
            };
            let index = window_state
                .tab_order
                .iter()
                .position(|candidate| *candidate == webview_id);
            window_state.tabs.remove(&webview_id);
            window_state.surfaces.remove(&webview_id);
            if let Some(index) = index {
                window_state.tab_order.remove(index);
            }
            // Show the rightmost remaining tab when the active one closes.
            let closed_active = window_state.active_tab == Some(webview_id);
            if closed_active {
                window_state.active_tab = window_state.tab_order.last().copied();
            }
            (
                window_state.tab_order.is_empty(),
                closed_active,
                window_state.window.clone(),
            )
        };
        if empty {
            window.close();
            return;
        }
        self.refresh_chrome(window_id);
        self.update_provider_viewport(window_id);
        if closed_active {
            // Present the new active tab's surface right away and request a
            // frame, so the closed tab's webview does not stay on screen
            // until the user picks another tab.
            let active = self
                .windows
                .get(&window_id)
                .and_then(|window_state| window_state.active_tab);
            if let Some(window_state) = self.windows.get_mut(&window_id) {
                Self::present_active_surface(window_state);
            }
            if let Some(provider) = self.provider.as_ref()
                && let Some(active) = active
                && let Err(error) = provider.frame_needed(active)
            {
                error!("[mac-embedder] frame needed: {error}");
            }
        }
    }

    fn action_navigate(&mut self, input: String) {
        let Some(url) = normalize_browser_destination(&input) else {
            return;
        };
        let Some(window_id) = self.active_window_id else {
            return;
        };
        let Some(webview_id) = self
            .windows
            .get(&window_id)
            .and_then(|window_state| window_state.active_tab)
        else {
            return;
        };
        if let Some(provider) = self.provider.as_ref() {
            let _ = provider.navigate(Some(webview_id), &url);
        }
        if let Some(window_state) = self.windows.get_mut(&window_id)
            && let Some(tab) = window_state.tabs.get_mut(&webview_id)
        {
            tab.pending_url = Some(url.clone());
        }
        self.refresh_address_field(window_id);
    }

    fn add_tab(&mut self, window_id: WindowId, webview_id: WebviewId) {
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        if window_state.tabs.contains_key(&webview_id) {
            window_state.active_tab = Some(webview_id);
            return;
        }
        window_state.tabs.insert(webview_id, TabState::new());
        window_state.tab_order.push(webview_id);
        window_state.active_tab = Some(webview_id);
    }

    /// Present the active tab's stored surface (the latest composited
    /// frame) on the web content layer.
    fn present_active_surface(window_state: &mut MacWindow) {
        let Some(webview_id) = window_state.active_tab else {
            return;
        };
        let Some(surface) = window_state.surfaces.get(&webview_id) else {
            return;
        };
        present_shared_surface(
            &window_state.web_layer,
            &surface.surface,
            surface.width,
            surface.padded_width,
            window_state.scale,
        );
    }

    // ── Windows ────────────────────────────────────────────────────────────

    fn create_window(&mut self, title: &str, destination: &str) -> Result<WindowId, String> {
        let mtm = self.mtm;
        // SAFETY: the window is created with the standard init; it is
        // retained by the app and released when closed (see below).
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(INITIAL_WINDOW_WIDTH, INITIAL_WINDOW_HEIGHT),
                ),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable
                    | NSWindowStyleMask::FullSizeContentView,
                objc2_app_kit::NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: the window is retained by the app; AppKit must not
        // release it when the window closes.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        window.setBackgroundColor(Some(&objc2_app_kit::NSColor::windowBackgroundColor()));
        window.setDelegate(Some(ProtocolObject::from_ref(&*self.delegate)));
        window.setAcceptsMouseMovedEvents(true);
        // The native toolbar sits in the titlebar (unified style) and the
        // web content extends behind it, so the chrome is not a separate
        // strip below the titlebar.
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setToolbarStyle(NSWindowToolbarStyle::UnifiedCompact);

        // The tab strip is its own row below the toolbar; the header-view
        // material makes it read as a continuation of the titlebar area.
        let tab_strip = NSVisualEffectView::new(mtm);
        tab_strip.setMaterial(NSVisualEffectMaterial::HeaderView);
        tab_strip.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
        tab_strip.setState(NSVisualEffectState::Active);
        let address_field = Self::make_address_field(mtm, &self.delegate);

        let window_id = WindowId::new();
        let scale = window.backingScaleFactor();
        let (web_view, web_layer) = new_layer_hosted_view(mtm, scale);

        // Each toolbar needs a unique identifier: customizable toolbars
        // are synchronized by it, and AppKit raises otherwise.
        let toolbar_identifier = NSString::from_str(&format!("formal-web-{}", window_id.0));
        let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &toolbar_identifier);
        toolbar.setDelegate(Some(ProtocolObject::from_ref(&*self.delegate)));
        toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        toolbar.setAllowsUserCustomization(true);

        let window_state = MacWindow {
            window: window.clone(),
            toolbar: toolbar.clone(),
            tab_strip,
            address_field,
            tab_cells: Vec::new(),
            tab_buttons: Vec::new(),
            tab_close_buttons: Vec::new(),
            web_view,
            web_layer,
            tabs: HashMap::new(),
            tab_order: Vec::new(),
            active_tab: None,
            hovered_tab: None,
            address_field_focused: false,
            surfaces: HashMap::new(),
            sublayers: HashMap::new(),
            keyboard_modifiers: KeyboardModifiers::default(),
            buttons: MouseEventButtons::None,
            content_size: (INITIAL_WINDOW_WIDTH, INITIAL_WINDOW_HEIGHT),
            scale,
            minimized: false,
        };
        // The window is registered before the toolbar is attached: the
        // toolbar delegate builds the address item from the window's view,
        // so the window must be findable in the map when AppKit queries
        // the delegate. Registering it as the active window before the
        // initial navigation starts also routes the user agent's
        // NewWebview and NavigationRequested events for the new traversable
        // to this window rather than the previously active one.
        self.windows.insert(window_id, window_state);
        self.active_window_id = Some(window_id);

        // Attach the toolbar, then measure and lay out the content area
        // below it (the tab strip row and the web content).
        window.setToolbar(Some(&toolbar));
        let content_size = if let Some(window_state) = self.windows.get_mut(&window_id) {
            let content_view = NSView::new(mtm);
            Self::layout_window_views(window_state);
            content_view.addSubview(&window_state.web_view);
            content_view.addSubview(&window_state.tab_strip);
            window.setContentView(Some(&content_view));
            window_state.content_size
        } else {
            (INITIAL_WINDOW_WIDTH, INITIAL_WINDOW_HEIGHT)
        };

        update_window_viewport_snapshot(Some(Self::viewport_tuple(content_size, scale)));
        if let Some(provider) = self.provider.as_mut() {
            let _ = provider.set_default_viewport(Some(Self::viewport_tuple(content_size, scale)));
        }

        window.center();
        window.makeKeyAndOrderFront(None);
        // AppKit's default key-view-loop focus would hand the first
        // responder to the native chrome (the address field starts
        // editing). A browser hands keystrokes to the web content by
        // default; clear the focus the window just assigned.
        window.makeFirstResponder(None);

        if let Some(provider) = self.provider.as_ref() {
            let _ = provider.navigate(None, destination);
        }

        Ok(window_id)
    }

    fn viewport_tuple(content_size: (f64, f64), scale: f64) -> (u32, u32, f32, ColorScheme) {
        let (width, height) = content_size;
        (
            (width * scale) as u32,
            (height * scale) as u32,
            scale as f32,
            ColorScheme::Light,
        )
    }

    fn window_for_webview(&self, webview_id: WebviewId) -> Option<WindowId> {
        self.windows.iter().find_map(|(window_id, window_state)| {
            if window_state.tabs.contains_key(&webview_id) {
                Some(*window_id)
            } else {
                None
            }
        })
    }

    fn close_window(&mut self, window_id: WindowId) {
        if let Some(window_state) = self.windows.get_mut(&window_id) {
            window_state.window.close();
        }
    }

    fn window_will_close(&mut self, notification: &NSNotification) {
        let Some(window_id) = self.window_id_for_notification(notification) else {
            return;
        };
        info!("[mac-embedder] window closed window={window_id:?}");
        if let Some(window_state) = self.windows.get_mut(&window_id) {
            window_state.tabs.clear();
            window_state.tab_order.clear();
            window_state.active_tab = None;
            window_state.surfaces.clear();
            window_state.tab_cells.clear();
            window_state.tab_buttons.clear();
            window_state.tab_close_buttons.clear();
            window_state.hovered_tab = None;
        }
        self.windows.remove(&window_id);
        if self.active_window_id == Some(window_id) {
            self.active_window_id = self.windows.keys().next().copied();
        }
        if self.windows.is_empty() {
            self.post_exit();
        }
    }

    fn window_did_resize(&mut self, notification: &NSNotification) {
        let Some(window_id) = self.window_id_for_notification(notification) else {
            return;
        };
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        window_state.scale = window_state.window.backingScaleFactor();
        Self::layout_window_views(window_state);
        let viewport = Self::viewport_tuple(window_state.content_size, window_state.scale);
        let webview_id = window_state.active_tab;

        update_window_viewport_snapshot(Some(viewport));
        if let Some(provider) = self.provider.as_mut() {
            let _ = provider.set_default_viewport(Some(viewport));
            if let Some(webview_id) = webview_id {
                let _ = provider.set_traversable_viewport(webview_id, viewport, 0.0, 0.0);
                let _ = provider.frame_needed(webview_id);
            }
        }
    }

    fn window_did_change_backing_properties(&mut self, notification: &NSNotification) {
        let Some(window_id) = self.window_id_for_notification(notification) else {
            return;
        };
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        window_state.scale = window_state.window.backingScaleFactor();
        let viewport = Self::viewport_tuple(window_state.content_size, window_state.scale);
        let webview_id = window_state.active_tab;

        update_window_viewport_snapshot(Some(viewport));
        if let Some(provider) = self.provider.as_mut()
            && let Some(webview_id) = webview_id
        {
            let _ = provider.set_traversable_viewport(webview_id, viewport, 0.0, 0.0);
            let _ = provider.frame_needed(webview_id);
        }
    }

    fn window_did_become_key(&mut self, notification: &NSNotification) {
        if let Some(window_id) = self.window_id_for_notification(notification) {
            self.active_window_id = Some(window_id);
        }
    }

    fn window_did_resign_key(&mut self, notification: &NSNotification) {
        if let Some(window_id) = self.window_id_for_notification(notification) {
            self.clear_tab_hover(window_id);
        }
    }

    fn window_did_miniaturize(&mut self, notification: &NSNotification) {
        if let Some(window_id) = self.window_id_for_notification(notification) {
            self.clear_tab_hover(window_id);
        }
        self.stop_display_link();
    }

    fn window_id_for_notification(&self, notification: &NSNotification) -> Option<WindowId> {
        let window = notification.object()?.downcast::<NSWindow>().ok()?;
        self.windows.iter().find_map(|(window_id, window_state)| {
            if std::ptr::eq(&*window, &*window_state.window) {
                Some(*window_id)
            } else {
                None
            }
        })
    }

    // ── Provider plumbing ──────────────────────────────────────────────────

    fn update_provider_viewport(&mut self, window_id: WindowId) {
        let Some(window_state) = self.windows.get(&window_id) else {
            return;
        };
        let viewport = Self::viewport_tuple(window_state.content_size, window_state.scale);
        let tab_webview_ids: Vec<WebviewId> = window_state.tab_order.clone();

        update_window_viewport_snapshot(Some(viewport));
        if let Some(provider) = self.provider.as_mut() {
            let _ = provider.set_default_viewport(Some(viewport));
            for tab_webview_id in tab_webview_ids {
                if let Err(error) =
                    provider.set_traversable_viewport(tab_webview_id, viewport, 0.0, 0.0)
                {
                    error!("[mac-embedder] set traversable viewport: {error}");
                }
            }
        }
    }

    fn dispatch_to_content(&mut self, window_id: WindowId, event: UiEvent) {
        let webview_id = self
            .windows
            .get(&window_id)
            .and_then(|window_state| window_state.active_tab);
        let Some(webview_id) = webview_id else {
            return;
        };
        if let Some(provider) = &self.provider
            && let Err(error) = provider.send_ui_event(webview_id, event)
        {
            error!("content event error: {error}");
        }
    }

    fn frame_needed(&self, webview_id: WebviewId) {
        if let Some(provider) = &self.provider
            && let Err(error) = provider.frame_needed(webview_id)
        {
            error!("[mac-embedder] frame needed: {error}");
        }
    }

    // ── User events ────────────────────────────────────────────────────────

    pub(crate) fn process_user_event(&mut self, event: FormalWebUserEvent) {
        if self.exiting {
            return;
        }
        match event {
            FormalWebUserEvent::RequestRedraw(webview_id) => {
                self.frame_needed(webview_id);
            }
            FormalWebUserEvent::NavigationRequested {
                webview_id,
                destination_url,
            } => {
                // A script-opened traversable is not a tab yet; it is
                // revealed on navigation commit.  Skip it here so the
                // unknown-webview branch below does not add it early.
                if self.pending_script_webviews.contains(&webview_id) {
                    return;
                }
                if let Some(window_id) = self.window_for_webview(webview_id) {
                    if let Some(window_state) = self.windows.get_mut(&window_id)
                        && let Some(tab) = window_state.tabs.get_mut(&webview_id)
                    {
                        tab.pending_url = Some(destination_url.clone());
                    }
                    if self
                        .windows
                        .get(&window_id)
                        .is_some_and(|w| w.active_tab == Some(webview_id))
                    {
                        self.refresh_chrome(window_id);
                        self.update_provider_viewport(window_id);
                    }
                } else if let Some(active_window) = self.active_window_id {
                    self.add_tab(active_window, webview_id);
                    self.refresh_chrome(active_window);
                    self.update_provider_viewport(active_window);
                }
            }
            FormalWebUserEvent::NavigationCompleted(completion) => {
                self.handle_navigation_completed(completion);
            }
            FormalWebUserEvent::NewWebview(webview_id, target_name) => {
                debug!(
                    "[mac-embedder] NewWebview webview={webview_id:?} target={}",
                    target_name
                );
                if target_name.is_empty() {
                    // Embedder-opened traversable (startup page, new-tab
                    // button): show the tab immediately.
                    if let Some(active_window) = self.active_window_id {
                        self.add_tab(active_window, webview_id);
                        self.refresh_chrome(active_window);
                        self.update_provider_viewport(active_window);
                    }
                } else {
                    // Script-opened traversable (window.open with a target
                    // name): defer until the navigation commits, so the
                    // intermediate about:blank document is never shown as
                    // a tab.
                    self.pending_script_webviews.insert(webview_id);
                }
            }
            FormalWebUserEvent::TitleChanged { webview_id, title } => {
                let update = {
                    let Some(window_id) = self.window_for_webview(webview_id) else {
                        return;
                    };
                    let Some(window_state) = self.windows.get_mut(&window_id) else {
                        return;
                    };
                    let Some(tab) = window_state.tabs.get_mut(&webview_id) else {
                        return;
                    };
                    tab.page_title = Some(title);
                    Some(window_id)
                };
                if let Some(window_id) = update {
                    self.refresh_chrome(window_id);
                }
            }
            FormalWebUserEvent::ClipboardRead { reply } => {
                let _ = reply.send(read_clipboard_text());
            }
            FormalWebUserEvent::ClipboardWrite { text, reply } => {
                let _ = reply.send(write_clipboard_text(text));
            }
            FormalWebUserEvent::NewWebContentLayers {
                webview_id,
                layers,
                animating,
            } => {
                self.handle_new_layers(webview_id, layers, animating);
            }
            FormalWebUserEvent::Automation(command) => {
                self.with_automation(|automation, app| {
                    automation.handle_command(app, command);
                });
            }
            FormalWebUserEvent::Exit => self.post_exit(),
        }
    }

    /// Run a closure with the automation controller temporarily moved out
    /// of `self`, so the command handler can borrow both the controller and
    /// the app (which implements `AutomationHost`) without a double borrow
    /// of `self`.
    fn with_automation<R>(
        &mut self,
        f: impl FnOnce(&mut AutomationController, &mut Self) -> R,
    ) -> R {
        let mut automation = std::mem::take(&mut self.automation);
        let result = f(&mut automation, self);
        self.automation = automation;
        result
    }

    fn handle_navigation_completed(&mut self, completion: NavigationCompleted) {
        // A script-opened traversable (window.open) becomes a tab when its
        // navigation commits, so the intermediate about:blank document is
        // never shown.  An aborted navigation leaves it unopened.
        if self
            .pending_script_webviews
            .contains(&completion.webview_id)
        {
            match &completion.status {
                NavigationCompletion::Committed { .. } => {
                    self.pending_script_webviews.remove(&completion.webview_id);
                    if let Some(active_window) = self.active_window_id {
                        self.add_tab(active_window, completion.webview_id);
                        self.refresh_chrome(active_window);
                        self.update_provider_viewport(active_window);
                        self.frame_needed(completion.webview_id);
                    }
                }
                NavigationCompletion::Aborted { .. } => {
                    self.pending_script_webviews.remove(&completion.webview_id);
                    return;
                }
            }
        }
        let Some(window_id) = self.window_for_webview(completion.webview_id) else {
            // Child traversables (iframes) fire their own completions and
            // don't create tabs.
            return;
        };
        let is_current = self
            .windows
            .get(&window_id)
            .is_some_and(|w| w.active_tab == Some(completion.webview_id));
        match &completion.status {
            NavigationCompletion::Committed { url } => {
                if let Some(window_state) = self.windows.get_mut(&window_id)
                    && let Some(tab) = window_state.tabs.get_mut(&completion.webview_id)
                {
                    tab.pending_url = None;
                    tab.committed_url = Some(url.clone());
                    // The previous document's title no longer applies; the
                    // new document reports its parsed title separately.
                    tab.page_title = None;
                }
                if let Some(provider) = self.provider.as_mut() {
                    provider.on_navigation_committed(completion.webview_id);
                }
                if is_current {
                    self.refresh_chrome(window_id);
                    self.update_provider_viewport(window_id);
                    self.frame_needed(completion.webview_id);
                }
            }
            NavigationCompletion::Aborted { .. } => {
                if is_current {
                    if let Some(window_state) = self.windows.get_mut(&window_id)
                        && let Some(tab) = window_state.tabs.get_mut(&completion.webview_id)
                    {
                        tab.pending_url = None;
                    }
                    self.refresh_chrome(window_id);
                }
            }
        }
    }

    fn handle_new_layers(
        &mut self,
        webview_id: WebviewId,
        layers: Vec<LayerFrame>,
        animating: bool,
    ) {
        let Some(window_id) = self.window_for_webview(webview_id) else {
            info!("[mac-embedder] no window for webview={webview_id:?}");
            return;
        };
        let Some(window_state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let is_active = window_state.active_tab == Some(webview_id);
        let scale = window_state.scale;

        // The compositor reports each layer's clip/transform in the root
        // navigable's local (pixel) coordinate space — the root surface is
        // 2400x1448 — while the web_layer and its sublayers live in window
        // point space (the window content size). Scale sublayer geometry by
        // (web_layer / root) so sublayers land where the root content is
        // drawn.
        let root_geometry = layers.iter().find(|layer| layer.topology.parent.is_none());
        let (root_w, root_h) = root_geometry
            .map(|layer| {
                (
                    f64::from(layer.topology.width),
                    f64::from(layer.topology.height),
                )
            })
            .unwrap_or((1.0, 1.0));
        let web_bounds = window_state.web_layer.bounds();
        let scale_x = web_bounds.size.width / root_w.max(1.0);
        let scale_y = web_bounds.size.height / root_h.max(1.0);
        debug!(
            "[mac-embedder] handle_layers webview={:?} root_size=({:.0}x{:.0}) web_layer_bounds=({:.0}x{:.0}) scale=({:.4},{:.4})",
            webview_id,
            root_w,
            root_h,
            web_bounds.size.width,
            web_bounds.size.height,
            scale_x,
            scale_y,
        );

        // The root navigable is presented on the existing web_layer, not as
        // a sublayer. Present the new surface only when this cycle
        // re-rendered it; a clean root keeps its last contents.
        let root = layers.iter().find(|layer| layer.topology.parent.is_none());
        if let Some(root) = root
            && let Some(frame) = &root.frame
        {
            let width = root.topology.width;
            let height = root.topology.height;
            // The zero-copy delivery path: the frame was rendered directly
            // into a shared IOSurface by the graphics process; the embedder
            // imports the surface and hands it to CoreAnimation.
            let SurfaceFrame::SharedTexture {
                surface_id, port, ..
            } = frame
            else {
                error!(
                    "[mac-embedder] received a CPU surface for webview={webview_id:?}; the AppKit embedder only supports the zero-copy shared-surface path"
                );
                return;
            };
            // Look the surface up by its global ID first: a surface object
            // imported from its Mach port (`IOSurfaceLookupFromMachPort`)
            // cannot be composited by CoreAnimation, while a by-ID lookup
            // (`IOSurfaceLookup`) of the same surface composites correctly.
            // Fall back to the port when the ID is not resolvable.
            let surface = IOSurfaceRef::lookup(*surface_id).or_else(|| {
                let port_name = port.clone().into_name();
                let surface = IOSurfaceRef::lookup_from_mach_port(port_name);
                deallocate_mach_port(port_name);
                surface
            });
            let Some(surface) = surface else {
                error!(
                    "[mac-embedder] IOSurfaceLookup failed for webview={webview_id:?} id={surface_id}"
                );
                return;
            };
            let surface_state = SurfaceState {
                surface,
                width,
                padded_width: padded_surface_width(width),
                animating,
            };
            if is_active {
                present_shared_surface(
                    &window_state.web_layer,
                    &surface_state.surface,
                    surface_state.width,
                    surface_state.padded_width,
                    window_state.scale,
                );
            }
            window_state.surfaces.insert(webview_id, surface_state);
            info!(
                "[mac-embedder] surface webview={webview_id:?} {}x{} animating={animating} active={is_active}",
                width, height
            );
        }

        // Child layers (cross-origin iframe navigables, video embed sites)
        // become sublayers under web_layer (or their parent's sublayer). One
        // transaction disables implicit animations for all moves/appearances.
        objc2_quartz_core::CATransaction::begin();
        objc2_quartz_core::CATransaction::setDisableActions(true);
        let mut seen = HashSet::new();
        for layer in &layers {
            let layer_id = layer.topology.layer_id;
            if layer.topology.parent.is_none() {
                continue; // root handled above
            }
            seen.insert(layer_id);

            // Resolve the parent CALayer: another sublayer if present, else
            // the web_layer (the root navigable). Retained clone ends the
            // sublayers borrow before the entry below.
            let parent_layer = layer
                .topology
                .parent
                .and_then(|parent_id| window_state.sublayers.get(&parent_id).cloned())
                .unwrap_or_else(|| window_state.web_layer.clone());

            if !window_state.sublayers.contains_key(&layer_id) {
                let sublayer = CALayer::layer();
                sublayer.setOpaque(true);
                sublayer.setContentsScale(scale);
                parent_layer.addSublayer(&sublayer);
                window_state.sublayers.insert(layer_id, sublayer);
            }
            let Some(sublayer) = window_state.sublayers.get(&layer_id) else {
                continue;
            };

            // Geometry: the layer's frame is its visible clip rect in its
            // parent's space, converted from root-pixel space to web_layer
            // point space; contents scale to fill (default
            // contentsGravity = resize), matching the producer's placement.
            //
            // The clip is reported in the root navigable's top-down pixel
            // space, while the web_layer (a layer-hosting view's backing
            // layer) has a bottom-up (y-up) coordinate space. Mapping the
            // top-down clip straight into the y-up frame would mirror the
            // sublayer vertically: it would sit above its element and move
            // opposite the page on scroll. Flip the y origin so sublayers
            // land on their element and track the page.
            let clip = &layer.topology.clip_bounds;
            let frame_y = web_bounds.size.height - clip[3] * scale_y;
            sublayer.setFrame(NSRect::new(
                NSPoint::new(clip[0] * scale_x, frame_y),
                NSSize::new((clip[2] - clip[0]) * scale_x, (clip[3] - clip[1]) * scale_y),
            ));
            sublayer.setCornerRadius(layer.topology.corner_radius);
            sublayer.setMasksToBounds(layer.topology.corner_radius > 0.0);
            let (z_index, paint_order) = layer.topology.z_order;
            sublayer.setZPosition((z_index as f64) * 10000.0 + (paint_order as f64));
            debug!(
                "[mac-embedder] sublayer {:?} parent={:?} frame={} clip={:?} z={:?} surface_size={:?} frame_pos={:?} frame_size={:?}",
                layer.topology.layer_id,
                layer.topology.parent,
                layer.frame.is_some(),
                layer.topology.clip_bounds,
                layer.topology.z_order,
                layer
                    .frame
                    .as_ref()
                    .map(|_| (layer.topology.width, layer.topology.height)),
                (clip[0] * scale_x, frame_y),
                ((clip[2] - clip[0]) * scale_x, (clip[3] - clip[1]) * scale_y,),
            );

            // Contents: only when re-rendered; a clean layer (frame: None)
            // keeps its last surface untouched.
            if let Some(frame) = &layer.frame {
                let SurfaceFrame::SharedTexture {
                    surface_id, port, ..
                } = frame
                else {
                    continue;
                };
                let surface = IOSurfaceRef::lookup(*surface_id).or_else(|| {
                    let port_name = port.clone().into_name();
                    let surface = IOSurfaceRef::lookup_from_mach_port(port_name);
                    deallocate_mach_port(port_name);
                    surface
                });
                if let Some(surface) = surface {
                    let padded_width = padded_surface_width(layer.topology.width);
                    // SAFETY: the sublayer retains the surface; the surface
                    // is a valid Objective-C object (an IOSurface).
                    let _: () = unsafe { msg_send![&**sublayer, setContents: &*surface] };
                    sublayer.setContentsRect(NSRect::new(
                        NSPoint::new(0.0, 0.0),
                        NSSize::new(
                            f64::from(layer.topology.width) / f64::from(padded_width.max(1)),
                            1.0,
                        ),
                    ));
                }
            }
        }
        // Drop sublayers whose layer no longer exists this cycle (navigable
        // torn down, video ended) — mirrors mark_child_frame_removed.
        let stale: Vec<_> = window_state
            .sublayers
            .iter()
            .filter(|(id, _)| !seen.contains(id))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            if let Some(layer) = window_state.sublayers.remove(&id) {
                debug!("[mac-embedder] remove sublayer {:?}", id);
                layer.removeFromSuperlayer();
            }
        }
        objc2_quartz_core::CATransaction::commit();

        // Pacing: animated content runs the display link; a static scene is
        // presented once and only re-renders on demand.
        if animating {
            self.start_display_link();
        } else {
            self.stop_display_link();
        }
    }
}

impl MacApp {
    /// The webview id of the active tab in the active window, if any.
    fn active_tab_webview_id(&self) -> Option<WebviewId> {
        let window_id = self.active_window_id?;
        self.windows
            .get(&window_id)
            .and_then(|window| window.active_tab)
    }

    /// Build pointer coordinates for an automation event from viewport
    /// (CSS-pixel) coordinates, matching the winit automation host.
    fn automation_coords(&self, x: f32, y: f32) -> PointerCoords {
        PointerCoords {
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
            page_x: x,
            page_y: y,
        }
    }
}

/// Capture the web content view (its layer tree: the root navigable on
/// `web_layer` plus the per-navigable/per-video sublayers) as a PNG.
/// Runs on the main thread; the view must be in a window.
fn capture_web_view_png(web_view: &NSView) -> Result<Vec<u8>, String> {
    let bounds = web_view.bounds();
    if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
        return Err(String::from("web content view has zero size"));
    }
    let bitmap = web_view
        .bitmapImageRepForCachingDisplayInRect(bounds)
        .ok_or_else(|| String::from("failed to allocate the screenshot bitmap"))?;
    web_view.cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap);
    let properties = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
    // SAFETY: the properties dictionary is empty, so there is no key/value
    // type mismatch for the `representationUsingType` call.
    let data = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
    }
    .ok_or_else(|| String::from("failed to encode the screenshot as PNG"))?;
    let mut buffer = vec![0u8; data.length() as usize];
    // SAFETY: `buffer` is exactly `data.length()` bytes and `getBytes_length`
    // writes exactly that many.
    unsafe {
        data.getBytes_length(
            NonNull::new(buffer.as_mut_ptr().cast::<c_void>())
                .expect("screenshot buffer is non-null"),
            buffer.len() as _,
        );
    }
    Ok(buffer)
}

/// Round a surface width up to a multiple of 64, the Metal constraint for
/// IOSurface-backed textures. Must match the producer's padding.
fn padded_surface_width(width: u32) -> u32 {
    (width.max(1) + 63) & !63
}

/// The kind of pointer event being dispatched.
#[derive(Clone, Copy)]
enum UiEventKind {
    PointerDown,
    PointerUp,
    PointerMove,
    Wheel,
}

/// Entry point: run the AppKit windowed app until it exits.
pub fn run_windowed_app(
    trace_sender: Option<TraceSender>,
    startup_url: Option<String>,
    window_title: Option<String>,
    cdp_port: Option<u16>,
) -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| String::from("the macOS embedder must run on the main thread"))?;
    MacApp::run(mtm, trace_sender, startup_url, window_title, cdp_port)
}
