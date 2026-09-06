use log::{debug, error, info};

use crate::chrome::{ChromeAction, ChromeTabInfo, ChromeUi, ChromeViewState};
use crate::events::{FormalWebUserEvent, send_user_event};
use crate::shared::{
    apple_standard_keybinding_for_key_down, automation_screenshot_png,
    normalize_browser_destination, read_clipboard_text, startup_destination_url,
    update_window_viewport_snapshot, write_clipboard_text,
};
use crate::winit_integration::WinitShellProvider;
use crate::winit_integration::{
    touch_pointer_details, viewport_of_snapshot, viewport_snapshot_for_window, winit_ime_to_blitz,
    winit_key_event_to_blitz, winit_modifiers_to_kbt_modifiers,
};
use anyrender::{PaintRef, PaintScene, RenderContext, ResourceId, WindowRenderer};
use anyrender_vello::VelloWindowRenderer;
use automation::{
    AutomationController, AutomationHost, AutomationSnapshot, AutomationVisibleFrameViewport,
};
use blitz_traits::SmolStr;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, MouseEventButton,
    MouseEventButtons, PointerCoords, PointerDetails, UiEvent,
};
use blitz_traits::shell::ShellProvider;
use kurbo::Affine;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;
#[cfg(target_os = "macos")]
use webview::deallocate_mach_port;
use webview::{NavigationCompletion, SurfaceFrame, WebviewId, WebviewProvider};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, PhysicalPosition};
use winit::event::{
    ElementState, Modifiers, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId as WinitWindowId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(Uuid);

impl WindowId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Per-tab state
pub(super) struct TabState {
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

/// Persistent GPU texture holding the latest composited surface pixels for
/// one webview, registered once with the Vello renderer. One variant per
/// delivery path: the CPU upload path fills the texture from shared-memory
/// bytes each frame; the zero-copy path (macOS) wraps a shared IOSurface.
pub(super) enum WebviewSurfaceTexture {
    /// CPU upload path: the texture is exactly the viewport size and its
    /// contents are replaced in place via `queue.write_texture`.
    CpuUpload {
        texture: wgpu::Texture,
        /// Vello registration id; valid while `registered` is true.
        resource_id: ResourceId,
        width: u32,
        height: u32,
        /// False until the texture has been registered with the renderer.
        /// Registration is dropped if the renderer is suspended; `paint_frame`
        /// re-registers unregistered textures before drawing.
        registered: bool,
    },
    /// macOS zero-copy path: the texture wraps a shared IOSurface imported
    /// from the graphics process's Mach port.
    #[cfg(target_os = "macos")]
    SharedSurface {
        texture: wgpu::Texture,
        /// Vello registration id; valid while `registered` is true.
        resource_id: ResourceId,
        /// Logical (viewport) width/height of the surface content.
        width: u32,
        height: u32,
        /// Physical width of the texture: the shared IOSurface is padded up
        /// to a 64-multiple width (Metal constraint); the draw clips to
        /// `width`.
        texture_width: u32,
        /// False until the texture has been registered with the renderer.
        /// Registration is dropped if the renderer is suspended; `paint_frame`
        /// re-registers unregistered textures before drawing.
        registered: bool,
        /// Identity of the shared IOSurface; changes on resize.
        texture_id: u64,
    },
}

impl WebviewSurfaceTexture {
    fn width(&self) -> u32 {
        match self {
            WebviewSurfaceTexture::CpuUpload { width, .. } => *width,
            #[cfg(target_os = "macos")]
            WebviewSurfaceTexture::SharedSurface { width, .. } => *width,
        }
    }

    fn height(&self) -> u32 {
        match self {
            WebviewSurfaceTexture::CpuUpload { height, .. } => *height,
            #[cfg(target_os = "macos")]
            WebviewSurfaceTexture::SharedSurface { height, .. } => *height,
        }
    }

    /// Physical texture width; padded for the shared surface variant.
    fn texture_width(&self) -> u32 {
        match self {
            WebviewSurfaceTexture::CpuUpload { width, .. } => *width,
            #[cfg(target_os = "macos")]
            WebviewSurfaceTexture::SharedSurface { texture_width, .. } => *texture_width,
        }
    }

    fn texture(&self) -> &wgpu::Texture {
        match self {
            WebviewSurfaceTexture::CpuUpload { texture, .. } => texture,
            #[cfg(target_os = "macos")]
            WebviewSurfaceTexture::SharedSurface { texture, .. } => texture,
        }
    }

    fn resource_id(&self) -> ResourceId {
        match self {
            WebviewSurfaceTexture::CpuUpload { resource_id, .. } => *resource_id,
            #[cfg(target_os = "macos")]
            WebviewSurfaceTexture::SharedSurface { resource_id, .. } => *resource_id,
        }
    }

    fn is_registered(&self) -> bool {
        match self {
            WebviewSurfaceTexture::CpuUpload { registered, .. } => *registered,
            #[cfg(target_os = "macos")]
            WebviewSurfaceTexture::SharedSurface { registered, .. } => *registered,
        }
    }

    fn set_registration(&mut self, resource_id: ResourceId, registered: bool) {
        match self {
            WebviewSurfaceTexture::CpuUpload {
                resource_id: id,
                registered: reg,
                ..
            } => {
                *id = resource_id;
                *reg = registered;
            }
            #[cfg(target_os = "macos")]
            WebviewSurfaceTexture::SharedSurface {
                resource_id: id,
                registered: reg,
                ..
            } => {
                *id = resource_id;
                *reg = registered;
            }
        }
    }

    /// Identity of the shared IOSurface this texture wraps; None on the CPU
    /// path. Changes on resize.
    #[cfg(target_os = "macos")]
    fn texture_id(&self) -> Option<u64> {
        match self {
            WebviewSurfaceTexture::CpuUpload { .. } => None,
            WebviewSurfaceTexture::SharedSurface { texture_id, .. } => Some(*texture_id),
        }
    }
}

/// Per-window state: owns a winit window, a renderer, chrome, and tabs
pub(super) struct WindowState {
    pub(super) window: Option<Arc<Window>>,
    pub(super) renderer: VelloWindowRenderer,
    /// Persistent GPU surface textures from the graphics process, keyed by
    /// webview.
    pub(super) surface_textures: HashMap<WebviewId, WebviewSurfaceTexture>,
    pub(super) chrome: Option<ChromeUi>,
    pub(super) tabs: HashMap<WebviewId, TabState>,
    pub(super) tab_order: Vec<WebviewId>,
    pub(super) active_tab: Option<WebviewId>,
    pub(super) automation: AutomationController,
    pub(super) window_occluded: bool,
    pub(super) animation_timer: Option<Instant>,
    pub(super) keyboard_modifiers: Modifiers,
    pub(super) buttons: MouseEventButtons,
    pub(super) pointer_pos: PhysicalPosition<f64>,
}

impl WindowState {
    fn new() -> Self {
        Self {
            window: None,
            renderer: VelloWindowRenderer::new(),
            surface_textures: HashMap::new(),
            chrome: None,
            tabs: HashMap::new(),
            tab_order: Vec::new(),
            active_tab: None,
            automation: AutomationController::default(),
            window_occluded: false,
            animation_timer: None,
            keyboard_modifiers: Modifiers::default(),
            buttons: MouseEventButtons::None,
            pointer_pos: PhysicalPosition::default(),
        }
    }
}

#[derive(Default)]
pub struct WindowedApp {
    pub(super) windows: HashMap<WindowId, WindowState>,
    pub(super) provider: Option<WebviewProvider>,
    pub(super) active_window_id: Option<WindowId>,
    /// Script-opened traversables (window.open) whose navigation has not
    /// committed yet.  They are not shown as tabs until the commit arrives,
    /// so the intermediate about:blank document is never visible.
    pub(super) pending_script_webviews: HashSet<WebviewId>,
    pub(super) startup_url: Option<String>,
    pub(super) window_title: Option<String>,
}

// ── Static helpers ─────────────────────────────────────────────────────────

impl WindowedApp {
    fn has_visible_viewport(window_state: &WindowState) -> bool {
        let Some(window) = window_state.window.as_ref() else {
            return false;
        };
        if window_state.window_occluded || matches!(window.is_visible(), Some(false)) {
            return false;
        }
        let size = window.inner_size();
        size.width > 0 && size.height > 0
    }

    fn chrome_height_css(window_state: &WindowState) -> f32 {
        window_state
            .chrome
            .as_ref()
            .map(ChromeUi::height_css)
            .unwrap_or_default()
    }

    fn chrome_height_physical(window_state: &WindowState) -> u32 {
        window_state
            .chrome
            .as_ref()
            .map(ChromeUi::height_physical)
            .unwrap_or_default()
    }

    fn content_has_visible_viewport(window_state: &WindowState) -> bool {
        Self::has_visible_viewport(window_state)
            && window_state.window.as_ref().is_some_and(|window| {
                window.inner_size().height > Self::chrome_height_physical(window_state)
            })
    }

    fn pointer_in_viewport(window_state: &WindowState, pos: PhysicalPosition<f64>) -> bool {
        Self::has_visible_viewport(window_state)
            && window_state.window.as_ref().is_some_and(|window| {
                let size = window.inner_size();
                pos.x >= 0.0
                    && pos.y >= 0.0
                    && pos.x < f64::from(size.width)
                    && pos.y < f64::from(size.height)
            })
    }

    fn pointer_in_chrome(window_state: &WindowState, pos: PhysicalPosition<f64>) -> bool {
        Self::pointer_in_viewport(window_state, pos)
            && pos.y < f64::from(Self::chrome_height_physical(window_state))
    }

    fn pointer_in_content(window_state: &WindowState, pos: PhysicalPosition<f64>) -> bool {
        Self::pointer_in_viewport(window_state, pos)
            && pos.y >= f64::from(Self::chrome_height_physical(window_state))
            && Self::content_has_visible_viewport(window_state)
    }

    fn request_window_redraw(window_state: &WindowState) {
        if let Some(window) = window_state.window.as_ref()
            && Self::has_visible_viewport(window_state)
        {
            window.request_redraw();
        }
    }

    fn request_visible_redraw(window_state: &WindowState) {
        Self::request_window_redraw(window_state);
    }

    fn tab_display_url(window_state: &WindowState) -> String {
        window_state
            .active_tab
            .and_then(|webview_id| window_state.tabs.get(&webview_id))
            .map(TabState::display_url)
            .unwrap_or_default()
    }

    fn tab_label(window_state: &WindowState, webview_id: &WebviewId) -> String {
        if let Some(tab) = window_state.tabs.get(webview_id) {
            // The tab label is the page title when available, falling back
            // to the URL for documents without one (and "New Tab" for
            // blank pages).
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
        }
        String::from("New Tab")
    }

    fn truncate_url(url: &str) -> String {
        let display = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .or_else(|| url.strip_prefix("file://"))
            .unwrap_or(url);
        if display.len() > 30 {
            format!("{}…", &display[..27])
        } else {
            display.to_owned()
        }
    }

    fn build_chrome_view_state(window_state: &WindowState) -> ChromeViewState {
        let tabs: Vec<ChromeTabInfo> = window_state
            .tab_order
            .iter()
            .map(|webview_id| ChromeTabInfo {
                label: Self::tab_label(window_state, webview_id),
                active: window_state.active_tab == Some(*webview_id),
            })
            .collect();
        ChromeViewState {
            address: Self::tab_display_url(window_state),
            tabs,
        }
    }

    fn sync_chrome(state: &mut WindowState) {
        let view_state = Self::build_chrome_view_state(state);
        if let Some(chrome) = state.chrome.as_mut() {
            chrome.sync_state(&view_state);
        }
    }

    fn update_provider_viewport(
        window_state: &WindowState,
        provider: &mut Option<WebviewProvider>,
    ) {
        let Some(window) = window_state.window.as_ref() else {
            return;
        };
        let (width, height, scale, color_scheme) = viewport_snapshot_for_window(window);
        let viewport = (
            width,
            height.saturating_sub(Self::chrome_height_physical(window_state)),
            scale,
            color_scheme,
        );
        update_window_viewport_snapshot(Some(viewport));
        if let Some(provider) = provider.as_mut() {
            // Apply the window's viewport to each top-level traversable in
            // THIS window. Broadcasting a single global default would resize
            // the content of every window: the UA applies the default to
            // whichever traversable is active, which may belong to a
            // different window. Per-traversable viewports keep each window's
            // content sized to its own window.
            for tab_webview_id in &window_state.tab_order {
                if let Err(error) =
                    provider.set_traversable_viewport(*tab_webview_id, viewport, 0.0, 0.0)
                {
                    error!("[embedder] set traversable viewport: {error}");
                }
            }
        }
    }

    fn logical_pos(window_state: &WindowState, pos: PhysicalPosition<f64>) -> LogicalPosition<f32> {
        let scale = window_state
            .window
            .as_ref()
            .map(|window| window.scale_factor())
            .unwrap_or(1.0);
        pos.to_logical(scale)
    }

    fn c_coords(window_state: &WindowState, pos: PhysicalPosition<f64>) -> PointerCoords {
        let LogicalPosition::<f32> { x, y } = Self::logical_pos(window_state, pos);
        PointerCoords {
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
            page_x: x,
            page_y: y,
        }
    }

    fn ct_coords(window_state: &WindowState, pos: PhysicalPosition<f64>) -> PointerCoords {
        let LogicalPosition::<f32> { x, y } = Self::logical_pos(window_state, pos);
        let chrome_height = Self::chrome_height_css(window_state);
        PointerCoords {
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y - chrome_height,
            page_x: x,
            page_y: y - chrome_height,
        }
    }

    fn add_tab(state: &mut WindowState, webview_id: WebviewId) {
        if state.tabs.contains_key(&webview_id) {
            state.active_tab = Some(webview_id);
            return;
        }
        state.tabs.insert(webview_id, TabState::new());
        state.tab_order.push(webview_id);
        state.active_tab = Some(webview_id);
    }

    fn paint_frame(state: &mut WindowState, provider: &Option<WebviewProvider>) {
        if !Self::has_visible_viewport(state) {
            return;
        }
        state.animation_timer.get_or_insert_with(Instant::now);
        let Some(window) = state.window.as_ref() else {
            return;
        };
        let chrome_height = f64::from(Self::chrome_height_physical(state));

        let chrome_scene = state.chrome.as_mut().map(ChromeUi::paint_scene);

        if chrome_scene.is_none() && state.active_tab.is_none() {
            return;
        }
        let size = window.inner_size();
        if state.renderer.is_active() {
            state.renderer.set_size(size.width, size.height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            state
                .renderer
                .resume(window_handle, size.width, size.height, || {});
            state.renderer.complete_resume();
        }
        window.pre_present_notify();

        // Re-register any surface textures whose Vello registration was
        // dropped (e.g. after a renderer suspend).
        for surface in state.surface_textures.values_mut() {
            if !surface.is_registered() {
                Self::register_surface_texture(&mut state.renderer, surface);
            }
        }

        // Notify the UA that a frame is needed before rendering, so the
        // next content render can start in parallel with this paint.
        if let Some(webview_id) = state.active_tab
            && let Some(provider) = provider.as_ref()
            && let Err(error) = provider.frame_needed(webview_id)
        {
            error!("[embedder] frame needed: {error}");
        }

        state.renderer.render(|scene| {
            // Paint content surface if one is available.
            let surface_found = state.active_tab.is_some_and(|webview_id| {
                if let Some(surface) = state.surface_textures.get(&webview_id)
                    && surface.is_registered()
                {
                    // Draw the texture at its natural (padded) size. While
                    // a resize is in flight the texture still holds the
                    // previous frame's dimensions, so the newly exposed area
                    // shows the surface base color until the next frame
                    // arrives; stretching it would look like the old content
                    // is being pulled out of shape. The shared IOSurface is
                    // padded to a 64-multiple width, so the padded region is
                    // clipped to the logical content rect.
                    let content_rect = kurbo::Rect::new(
                        0.0,
                        0.0,
                        f64::from(surface.width()),
                        f64::from(surface.height()),
                    );
                    let texture_rect = kurbo::Rect::new(
                        0.0,
                        0.0,
                        f64::from(surface.texture_width()),
                        f64::from(surface.height()),
                    );
                    let content_transform = Affine::translate((0.0, chrome_height));
                    scene.push_clip_layer(content_transform, &content_rect);
                    scene.fill(
                        peniko::Fill::NonZero,
                        content_transform,
                        PaintRef::Resource(peniko::ImageBrush {
                            image: surface.resource_id(),
                            sampler: Default::default(),
                        }),
                        None,
                        &texture_rect,
                    );
                    scene.pop_layer();
                    info!(
                        "[render-pipe] Embedder paint webview={:?} {}x{} at y={}",
                        webview_id,
                        surface.width(),
                        surface.height(),
                        chrome_height
                    );
                    true
                } else {
                    info!(
                        "[render-pipe] Embedder paint NO SURFACE for active_tab={:?}",
                        webview_id
                    );
                    false
                }
            });
            if !surface_found {
                info!("[render-pipe] Embedder paint chrome-only");
            }
            // Always paint chrome, even when no content surface is available
            // (e.g. chrome-only hover, typing in address bar).
            if let Some(chrome_scene) = chrome_scene.clone() {
                scene.append_scene(chrome_scene, Affine::IDENTITY);
            }
        });
    }

    fn window_for_webview(app: &Self, webview_id: WebviewId) -> Option<WindowId> {
        app.windows.iter().find_map(|(window_id, window_state)| {
            if window_state.tabs.contains_key(&webview_id) {
                Some(*window_id)
            } else {
                None
            }
        })
    }

    fn auto_snapshot(window_state: &WindowState) -> AutomationSnapshot {
        AutomationSnapshot {
            webview_id: window_state.active_tab,
            current_url: window_state
                .active_tab
                .and_then(|webview_id| window_state.tabs.get(&webview_id))
                .and_then(|tab| tab.committed_url.clone()),
            displayed_url: Self::tab_display_url(window_state),
            navigable_id: None,
            has_top_level_traversable: window_state.active_tab.is_some(),
        }
    }

    fn create_winit_window(
        event_loop: &ActiveEventLoop,
        window_title: Option<String>,
    ) -> Result<Arc<Window>, String> {
        let title = window_title.unwrap_or_else(|| String::from("formal-web"));
        event_loop
            .create_window(Window::default_attributes().with_title(title))
            .map(Arc::new)
            .map_err(|error| format!("failed to create winit window: {error}"))
    }

    /// Create the persistent GPU texture that receives composited surface
    /// pixels for one webview. `COPY_SRC` is required by Vello's
    /// `register_texture`, which copies the texture into its image atlas.
    fn create_surface_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("webview-surface"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// Register a surface texture with the Vello renderer, keeping the
    /// `ResourceId` stable across frames. No-op while the renderer is not
    /// active; `paint_frame` retries for unregistered textures.
    fn register_surface_texture(
        renderer: &mut VelloWindowRenderer,
        surface: &mut WebviewSurfaceTexture,
    ) {
        if !renderer.is_active() {
            return;
        }
        match renderer.try_register_custom_resource(Box::new(surface.texture().clone())) {
            Ok(resource_id) => {
                surface.set_registration(resource_id, true);
            }
            Err(error) => error!("[embedder] register surface texture: {error:?}"),
        }
    }

    /// Round a surface width up to a multiple of 64, the Metal constraint for
    /// IOSurface-backed textures. Must match the producer's padding.
    #[cfg(target_os = "macos")]
    fn padded_surface_width(width: u32) -> u32 {
        (width.max(1) + 63) & !63
    }

    /// Import a shared IOSurface (macOS zero-copy path) as a wgpu texture on
    /// the embedder's device. The consumer only reads the surface, so the
    /// usage is `TEXTURE_BINDING | COPY_SRC` (what Vello's `register_texture`
    /// requires). `width` must be the padded surface width (multiple of 64).
    #[cfg(target_os = "macos")]
    fn import_shared_surface(
        device: &wgpu::Device,
        surface: &objc2_io_surface::IOSurfaceRef,
        width: u32,
        height: u32,
    ) -> Option<wgpu::Texture> {
        use objc2::rc::Retained;
        use objc2::runtime::ProtocolObject;
        use objc2_metal::{
            MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTexture, MTLTextureDescriptor,
            MTLTextureUsage,
        };

        // SAFETY: the hal device is this renderer's own device; the raw
        // Metal device is used only to create textures on it.
        let hal_device = unsafe { device.as_hal::<wgpu::hal::metal::Api>() }?;
        let raw_device = hal_device.raw_device();
        let descriptor = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::RGBA8Unorm,
                width as usize,
                height as usize,
                false,
            )
        };
        descriptor.setStorageMode(MTLStorageMode::Private);
        descriptor.setUsage(MTLTextureUsage::ShaderRead);
        let metal_texture =
            raw_device.newTextureWithDescriptor_iosurface_plane(&descriptor, surface, 0)?;
        let metal_texture: Retained<ProtocolObject<dyn MTLTexture>> =
            ProtocolObject::from_retained(metal_texture);
        let hal_texture = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                metal_texture,
                wgpu::TextureFormat::Rgba8Unorm,
                objc2_metal::MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width,
                    height,
                    depth: 1,
                },
            )
        };
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu::hal::metal::Api>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("webview-shared-surface"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                },
            )
        };
        Some(texture)
    }

    fn resume_renderer(state: &mut WindowState, window: &Arc<Window>) {
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if state.renderer.is_active() {
            state.renderer.set_size(size.width, size.height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            state
                .renderer
                .resume(window_handle, size.width, size.height, || {});
            state.renderer.complete_resume();
        }
    }

    fn with_provider<R>(&mut self, callback: impl FnOnce(&mut WebviewProvider) -> R) -> Option<R> {
        self.provider.as_mut().map(callback)
    }

    fn dispatch_to_content(&mut self, window_id: WindowId, event: UiEvent) {
        let Some(webview_id) = self
            .windows
            .get(&window_id)
            .and_then(|window_state| window_state.active_tab)
        else {
            return;
        };
        self.with_provider(|provider| {
            if let Err(error) = provider.send_ui_event(webview_id, event) {
                error!("content event error: {error}");
            }
        });
    }

    fn try_run_automation<R>(
        &mut self,
        automation_fn: impl FnOnce(&mut AutomationController, &mut WindowedApp) -> R,
    ) -> Option<R> {
        let window_id = self.active_window_id?;
        let state = self.windows.get_mut(&window_id)?;
        let mut automation = std::mem::take(&mut state.automation);
        let result = automation_fn(&mut automation, self);
        if let Some(state) = self.windows.get_mut(&window_id) {
            state.automation = automation;
        }
        Some(result)
    }
}

// ── ApplicationHandler ─────────────────────────────────────────────────────

impl ApplicationHandler<FormalWebUserEvent> for WindowedApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        let window_id = WindowId::new();
        let window = match Self::create_winit_window(event_loop, self.window_title.clone()) {
            Ok(window) => window,
            Err(_) => {
                event_loop.exit();
                return;
            }
        };
        let viewport = viewport_of_snapshot(viewport_snapshot_for_window(&window));
        let shell_provider: Arc<dyn ShellProvider> =
            Arc::new(WinitShellProvider::new(window.clone()));
        let chrome = match ChromeUi::new(viewport, shell_provider) {
            Ok(chrome) => chrome,
            Err(_) => {
                event_loop.exit();
                return;
            }
        };
        let mut state = WindowState::new();
        state.chrome = Some(chrome);
        state.window = Some(window.clone());
        self.active_window_id = Some(window_id);
        Self::sync_chrome(&mut state);
        // Set default viewport so new traversables know initial dimensions.
        // Don't call set_traversable_viewport — no tab exists yet.
        if let Some(window) = state.window.as_ref() {
            let (width, height, scale, color_scheme) = viewport_snapshot_for_window(window);
            let viewport = (
                width,
                height.saturating_sub(Self::chrome_height_physical(&state)),
                scale,
                color_scheme,
            );
            update_window_viewport_snapshot(Some(viewport));
            if let Some(provider) = self.provider.as_mut() {
                let _ = provider.set_default_viewport(Some(viewport));
            }
        }
        Self::resume_renderer(&mut state, &window);
        // Determine destination URL: provided startup URL, artifact, or fallback.
        let destination = startup_destination_url(self.startup_url.as_deref())
            .unwrap_or_else(|_| String::from("about:blank"));
        if let Some(provider) = self.provider.as_ref() {
            let _ = provider.navigate(None, &destination);
        }

        self.windows.insert(window_id, state);
        if let Some(window_state) = self.windows.get(&window_id) {
            Self::request_window_redraw(window_state);
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        winit_window_id: WinitWindowId,
        event: WindowEvent,
    ) {
        let window_id = match self
            .windows
            .iter()
            .find(|(_, window_state)| {
                window_state.window.as_ref().map(|window| window.id()) == Some(winit_window_id)
            })
            .map(|(id, _)| *id)
        {
            Some(id) => id,
            None => return,
        };
        self.active_window_id = Some(window_id);

        match event {
            WindowEvent::RedrawRequested => {
                if let Some(state) = self.windows.get_mut(&window_id)
                    && (self.provider.is_some() || state.chrome.is_some())
                {
                    Self::paint_frame(state, &self.provider);
                }
            }
            WindowEvent::Occluded(occluded) => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.window_occluded = occluded;
                }
                // The window became visible again; request a redraw so
                // painting resumes.
                if !occluded && let Some(state) = self.windows.get(&window_id) {
                    Self::request_window_redraw(state);
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    if let Some(window) = state.window.as_ref() {
                        let viewport = viewport_of_snapshot(viewport_snapshot_for_window(window));
                        if let Some(chrome) = state.chrome.as_mut() {
                            chrome.set_viewport(viewport);
                        }
                        Self::sync_chrome(state);
                    }
                    if state.renderer.is_active() {
                        state.renderer.set_size(size.width, size.height);
                    }
                    // The chrome (and the whole scene) needs repainting at
                    // the new size.
                    Self::request_window_redraw(state);
                }
                // Update provider viewport (separate borrow from state)
                if let Some(state) = self.windows.get(&window_id) {
                    Self::update_provider_viewport(state, &mut self.provider);
                }
            }
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    if let Some(window) = state.window.as_ref() {
                        window.set_visible(false);
                    }
                    state
                        .automation
                        .abort_pending_navigation(String::from("window closed"));
                    state.renderer.suspend();
                    state.animation_timer = None;
                    state.chrome = None;
                    state.tabs.clear();
                    state.tab_order.clear();
                    state.active_tab = None;
                    state.window_occluded = false;
                    state.window = None;
                }
                // Update active window if the closed one was active
                if self.active_window_id == Some(window_id) {
                    self.active_window_id = self.windows.keys().next().copied();
                }
                self.windows.remove(&window_id);
                if self.windows.is_empty() {
                    let _ = send_user_event(FormalWebUserEvent::Exit);
                }
            }
            WindowEvent::Ime(event) => {
                let ui_event = UiEvent::Ime(winit_ime_to_blitz(event));
                if Self::is_chrome_focused(&self.windows, window_id) {
                    Self::chrome_event(self, window_id, ui_event);
                } else {
                    self.dispatch_to_content(window_id, ui_event);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.keyboard_modifiers = modifiers;
                }
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                let modifiers = self
                    .windows
                    .get(&window_id)
                    .map(|state| state.keyboard_modifiers.state())
                    .unwrap_or_default();
                let key = winit_key_event_to_blitz(&key_event, modifiers);
                let apple_standard_keybinding = apple_standard_keybinding_for_key_down(&key);
                let ui_event = if key_event.state.is_pressed() {
                    UiEvent::KeyDown(key)
                } else {
                    UiEvent::KeyUp(key)
                };
                let chrome_focused = Self::is_chrome_focused(&self.windows, window_id);
                if chrome_focused {
                    if let Some(command) = apple_standard_keybinding {
                        Self::chrome_event(
                            self,
                            window_id,
                            UiEvent::AppleStandardKeybinding(SmolStr::new(command)),
                        );
                    } else {
                        Self::chrome_event(self, window_id, ui_event);
                    }
                } else {
                    self.dispatch_to_content(window_id, ui_event);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if Self::pointer_in_chrome_st(&self.windows, window_id, position) {
                    if let Some(state) = self.windows.get_mut(&window_id) {
                        state.pointer_pos = position;
                        let event = UiEvent::PointerMove(BlitzPointerEvent {
                            id: BlitzPointerId::Mouse,
                            is_primary: true,
                            coords: Self::c_coords(state, position),
                            button: Default::default(),
                            buttons: state.buttons,
                            mods: winit_modifiers_to_kbt_modifiers(
                                state.keyboard_modifiers.state(),
                            ),
                            details: PointerDetails::default(),
                        });
                        Self::chrome_event(self, window_id, event);
                    }
                } else if Self::pointer_in_content_st(&self.windows, window_id, position)
                    && let Some(state) = self.windows.get_mut(&window_id)
                {
                    state.pointer_pos = position;
                    let coords = Self::ct_coords(state, position);
                    let buttons = state.buttons;
                    let modifiers =
                        winit_modifiers_to_kbt_modifiers(state.keyboard_modifiers.state());
                    self.dispatch_to_content(
                        window_id,
                        UiEvent::PointerMove(BlitzPointerEvent {
                            id: BlitzPointerId::Mouse,
                            is_primary: true,
                            coords,
                            button: Default::default(),
                            buttons,
                            mods: modifiers,
                            details: PointerDetails::default(),
                        }),
                    );
                }
            }
            WindowEvent::MouseInput {
                button,
                state: button_state,
                ..
            } => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    if !Self::pointer_in_viewport(state, state.pointer_pos) {
                        return;
                    }
                    let mouse_button = Self::map_button(button);
                    match button_state {
                        ElementState::Pressed => state.buttons |= mouse_button.into(),
                        ElementState::Released => state.buttons.remove(mouse_button.into()),
                    }
                    if Self::pointer_in_chrome(state, state.pointer_pos) {
                        let event = BlitzPointerEvent {
                            id: BlitzPointerId::Mouse,
                            is_primary: true,
                            coords: Self::c_coords(state, state.pointer_pos),
                            button: mouse_button,
                            buttons: state.buttons,
                            mods: winit_modifiers_to_kbt_modifiers(
                                state.keyboard_modifiers.state(),
                            ),
                            details: PointerDetails::default(),
                        };
                        Self::chrome_event(
                            self,
                            window_id,
                            match button_state {
                                ElementState::Pressed => UiEvent::PointerDown(event),
                                ElementState::Released => UiEvent::PointerUp(event),
                            },
                        );
                    } else if Self::pointer_in_content(state, state.pointer_pos) {
                        if button_state.is_pressed() {
                            if let Some(chrome) = state.chrome.as_mut() {
                                chrome.clear_focus();
                            }
                            // The chrome loses text-input focus: repaint the
                            // chrome's focus styling.
                            Self::request_window_redraw(state);
                        }
                        let event = BlitzPointerEvent {
                            id: BlitzPointerId::Mouse,
                            is_primary: true,
                            coords: Self::ct_coords(state, state.pointer_pos),
                            button: mouse_button,
                            buttons: state.buttons,
                            mods: winit_modifiers_to_kbt_modifiers(
                                state.keyboard_modifiers.state(),
                            ),
                            details: PointerDetails::default(),
                        };
                        let webview_id = state.active_tab;
                        if let Some(webview_id) = webview_id {
                            self.with_provider(|provider| {
                                let result = match button_state {
                                    ElementState::Pressed => provider
                                        .send_ui_event(webview_id, UiEvent::PointerDown(event)),
                                    ElementState::Released => provider
                                        .send_ui_event(webview_id, UiEvent::PointerUp(event)),
                                };
                                if let Err(error) = result {
                                    error!("content event error: {error}");
                                }
                            });
                        }
                    }
                }
            }
            WindowEvent::Touch(Touch {
                phase,
                location,
                force,
                id,
                ..
            }) => {
                let in_chrome = Self::pointer_in_chrome_st(&self.windows, window_id, location);
                if let Some(state) = self.windows.get_mut(&window_id) {
                    if !Self::pointer_in_viewport(state, location) {
                        return;
                    }
                    let event = BlitzPointerEvent {
                        id: BlitzPointerId::Finger(id),
                        is_primary: true,
                        coords: if in_chrome {
                            Self::c_coords(state, location)
                        } else {
                            Self::ct_coords(state, location)
                        },
                        button: Default::default(),
                        buttons: MouseEventButtons::None,
                        mods: winit_modifiers_to_kbt_modifiers(state.keyboard_modifiers.state()),
                        details: touch_pointer_details(force),
                    };
                    if in_chrome {
                        Self::chrome_event(
                            self,
                            window_id,
                            match phase {
                                TouchPhase::Started => UiEvent::PointerDown(event),
                                TouchPhase::Moved => UiEvent::PointerMove(event),
                                TouchPhase::Ended | TouchPhase::Cancelled => {
                                    UiEvent::PointerUp(event)
                                }
                            },
                        );
                    } else if Self::pointer_in_content(state, location) {
                        if phase == TouchPhase::Started {
                            if let Some(chrome) = state.chrome.as_mut() {
                                chrome.clear_focus();
                            }
                            // The chrome loses text-input focus: repaint the
                            // chrome's focus styling.
                            Self::request_window_redraw(state);
                        }
                        let webview_id = state.active_tab;
                        if let Some(webview_id) = webview_id {
                            self.with_provider(|provider| {
                                let result = match phase {
                                    TouchPhase::Started => provider
                                        .send_ui_event(webview_id, UiEvent::PointerDown(event)),
                                    TouchPhase::Moved => provider
                                        .send_ui_event(webview_id, UiEvent::PointerMove(event)),
                                    TouchPhase::Ended | TouchPhase::Cancelled => provider
                                        .send_ui_event(webview_id, UiEvent::PointerUp(event)),
                                };
                                if let Err(error) = result {
                                    error!("touch event error: {error}");
                                }
                            });
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let wheel_delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => BlitzWheelDelta::Lines(x as f64, y as f64),
                    MouseScrollDelta::PixelDelta(pixel_delta) => {
                        BlitzWheelDelta::Pixels(pixel_delta.x, pixel_delta.y)
                    }
                };
                let pointer_in_chrome = Self::pointer_in_chrome_st(
                    &self.windows,
                    window_id,
                    self.windows
                        .get(&window_id)
                        .map_or(PhysicalPosition::default(), |state| state.pointer_pos),
                );
                if pointer_in_chrome {
                    if let Some(state) = self.windows.get_mut(&window_id) {
                        if !Self::pointer_in_viewport(state, state.pointer_pos) {
                            return;
                        }
                        let event = UiEvent::Wheel(BlitzWheelEvent {
                            delta: wheel_delta,
                            coords: Self::c_coords(state, state.pointer_pos),
                            buttons: state.buttons,
                            mods: winit_modifiers_to_kbt_modifiers(
                                state.keyboard_modifiers.state(),
                            ),
                        });
                        Self::chrome_event(self, window_id, event);
                    }
                } else if let Some(state) = self.windows.get_mut(&window_id) {
                    if !Self::pointer_in_viewport(state, state.pointer_pos) {
                        return;
                    }
                    let webview_id = state.active_tab;
                    let coords = Self::ct_coords(state, state.pointer_pos);
                    let buttons = state.buttons;
                    let modifiers =
                        winit_modifiers_to_kbt_modifiers(state.keyboard_modifiers.state());
                    if let Some(webview_id) = webview_id {
                        self.with_provider(|provider| {
                            if let Err(error) = provider.send_ui_event(
                                webview_id,
                                UiEvent::Wheel(BlitzWheelEvent {
                                    delta: wheel_delta,
                                    coords,
                                    buttons,
                                    mods: modifiers,
                                }),
                            ) {
                                error!("wheel event error: {error}");
                            }
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: FormalWebUserEvent) {
        match event {
            FormalWebUserEvent::RequestRedraw(webview_id) => {
                if let Some(window) = Self::window_for_webview(self, webview_id) {
                    let is_active = self
                        .windows
                        .get(&window)
                        .is_some_and(|state| state.active_tab == Some(webview_id));
                    if is_active && let Some(window_state) = self.windows.get(&window) {
                        Self::request_window_redraw(window_state);
                    }
                }
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
                // Update pending URL for any known webview (active tab or not).
                // If the webview is not in any window, create a new tab for it.
                if let Some(window) = Self::window_for_webview(self, webview_id) {
                    if let Some(state) = self.windows.get_mut(&window) {
                        if let Some(tab) = state.tabs.get_mut(&webview_id) {
                            tab.pending_url = Some(destination_url.clone());
                        }
                        if state.active_tab == Some(webview_id) {
                            Self::sync_chrome(state);
                            Self::update_provider_viewport(state, &mut self.provider);
                            Self::request_window_redraw(state);
                        }
                    }
                } else if let Some(active_window) = self.active_window_id
                    && let Some(state) = self.windows.get_mut(&active_window)
                {
                    Self::add_tab(state, webview_id);
                    Self::sync_chrome(state);
                    Self::update_provider_viewport(state, &mut self.provider);
                }
            }
            FormalWebUserEvent::NavigationCompleted(completion) => {
                // A script-opened traversable (window.open) becomes a tab
                // when its navigation commits, so the intermediate
                // about:blank document is never shown.  An aborted
                // navigation leaves it unopened.
                if self
                    .pending_script_webviews
                    .contains(&completion.webview_id)
                {
                    match &completion.status {
                        NavigationCompletion::Committed { .. } => {
                            self.pending_script_webviews.remove(&completion.webview_id);
                            if let Some(active_window) = self.active_window_id
                                && let Some(state) = self.windows.get_mut(&active_window)
                            {
                                Self::add_tab(state, completion.webview_id);
                                Self::sync_chrome(state);
                                Self::update_provider_viewport(state, &mut self.provider);
                                Self::request_visible_redraw(state);
                            }
                        }
                        NavigationCompletion::Aborted { .. } => {
                            self.pending_script_webviews.remove(&completion.webview_id);
                            return;
                        }
                    }
                }
                let window_opt = Self::window_for_webview(self, completion.webview_id);
                let Some(window) = window_opt else {
                    // Ignore: child traversables (iframes) fire their own
                    // NavigationCompleted — those don't create tabs.
                    return;
                };
                let is_current = self.windows.get(&window).is_some_and(|window_state| {
                    window_state.active_tab == Some(completion.webview_id)
                });
                match &completion.status {
                    NavigationCompletion::Committed { url } => {
                        if let Some(state) = self.windows.get_mut(&window)
                            && let Some(tab) = state.tabs.get_mut(&completion.webview_id)
                        {
                            tab.pending_url = None;
                            tab.committed_url = Some(url.clone());
                            // The previous document's title no longer
                            // applies; the new document reports its parsed
                            // title separately.
                            tab.page_title = None;
                        }
                        // Clear compositor first so new paint frames populate it.
                        if let Some(provider) = self.provider.as_mut() {
                            provider.on_navigation_committed(completion.webview_id);
                        }
                        if is_current {
                            if let Some(state) = self.windows.get_mut(&window) {
                                Self::sync_chrome(state);
                                Self::update_provider_viewport(state, &mut self.provider);
                                Self::request_window_redraw(state);
                            }
                            self.try_run_automation(|automation, app| {
                                automation.note_navigation_committed(app)
                            });
                        }
                    }
                    NavigationCompletion::Aborted { message } => {
                        if is_current && let Some(state) = self.windows.get_mut(&window) {
                            let mut automation = std::mem::take(&mut state.automation);
                            automation.abort_pending_navigation(message.clone());
                            state.automation = automation;
                            if let Some(tab) = state.tabs.get_mut(&completion.webview_id) {
                                tab.pending_url = None;
                            }
                            Self::sync_chrome(state);
                            Self::request_window_redraw(state);
                        }
                    }
                }
            }
            FormalWebUserEvent::NewWebview(webview_id, target_name) => {
                debug!(
                    "[embedder] NewWebview webview={:?} target={}",
                    webview_id, target_name
                );
                if target_name.is_empty() {
                    // Embedder-opened traversable (startup page, new-tab
                    // button): show the tab immediately.
                    if let Some(active_window) = self.active_window_id
                        && let Some(state) = self.windows.get_mut(&active_window)
                    {
                        Self::add_tab(state, webview_id);
                        Self::sync_chrome(state);
                        Self::update_provider_viewport(state, &mut self.provider);
                        Self::request_visible_redraw(state);
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
                let window_opt = Self::window_for_webview(self, webview_id);
                if let Some(window) = window_opt
                    && let Some(state) = self.windows.get_mut(&window)
                    && let Some(tab) = state.tabs.get_mut(&webview_id)
                {
                    tab.page_title = Some(title);
                    Self::sync_chrome(state);
                    Self::request_visible_redraw(state);
                }
            }
            FormalWebUserEvent::CreateWindow => {
                let window_id = WindowId::new();
                let window = match Self::create_winit_window(event_loop, self.window_title.clone())
                {
                    Ok(window) => window,
                    Err(_) => return,
                };
                let viewport = viewport_of_snapshot(viewport_snapshot_for_window(&window));
                let shell_provider: Arc<dyn ShellProvider> =
                    Arc::new(WinitShellProvider::new(window.clone()));
                let chrome = match ChromeUi::new(viewport, shell_provider) {
                    Ok(chrome) => chrome,
                    Err(_) => return,
                };
                let mut state = WindowState::new();
                state.chrome = Some(chrome);
                state.window = Some(window.clone());
                self.active_window_id = Some(window_id);
                Self::sync_chrome(&mut state);
                Self::update_provider_viewport(&state, &mut self.provider);
                Self::resume_renderer(&mut state, &window);
                if let Some(provider) = self.provider.as_ref() {
                    let _ = provider.navigate(None, "about:blank");
                }
                self.windows.insert(window_id, state);
            }
            FormalWebUserEvent::Automation(command) => {
                self.try_run_automation(|automation, app| automation.handle_command(app, command));
            }
            FormalWebUserEvent::ClipboardRead { reply } => {
                let _ = reply.send(read_clipboard_text());
            }
            FormalWebUserEvent::ClipboardWrite { text, reply } => {
                let _ = reply.send(write_clipboard_text(text));
            }
            FormalWebUserEvent::NewWebContentScene { webview_id, .. } => {
                // Removed: the IOSurface surface path replaces the old scene bytes path.
                // Trigger a redraw so the surface-based path can render.
                debug!(
                    "[embedder] NewWebContentScene (ignored, using surface path) webview={:?}",
                    webview_id,
                );
                if let Some(window) = Self::window_for_webview(self, webview_id)
                    && let Some(state) = self.windows.get(&window)
                {
                    Self::request_window_redraw(state);
                }
            }
            FormalWebUserEvent::NewWebContentLayers {
                webview_id, layers, ..
            } => {
                // Temporary: draw only the root navigable layer until the
                // per-layer quad path lands (step 7).
                let Some(root) = layers
                    .into_iter()
                    .find(|layer| layer.topology.parent.is_none())
                else {
                    return;
                };
                let Some(frame) = root.frame else {
                    return;
                };
                let width = root.topology.width;
                let height = root.topology.height;
                let Some(window_id) = Self::window_for_webview(self, webview_id) else {
                    info!(
                        "[render-pipe] Embedder no window for webview={:?}",
                        webview_id
                    );
                    return;
                };
                let Some(state) = self.windows.get_mut(&window_id) else {
                    return;
                };
                // The renderer must be active (window resumed) to create and
                // register GPU textures. If not, drop the frame — the next
                // one arrives once the window is rendering again.
                let Some(device_handle) = state.renderer.current_device_handle().cloned() else {
                    info!(
                        "[render-pipe] Embedder renderer inactive, dropping surface webview={:?}",
                        webview_id
                    );
                    return;
                };

                match frame {
                    SurfaceFrame::CpuShmem(surface) => {
                        let total_pixels = (width * height * 4) as usize;
                        let pixels = surface.as_slice();
                        info!(
                            "[render-pipe] Embedder surface webview={:?} {}x{} pixels={}B active_tab={:?}",
                            webview_id, width, height, total_pixels, state.active_tab
                        );
                        if pixels.len() < total_pixels || width == 0 || height == 0 {
                            info!(
                                "[render-pipe] Embedder invalid surface webview={:?} {}x{} shmem={}B (expected {}B)",
                                webview_id,
                                width,
                                height,
                                pixels.len(),
                                total_pixels
                            );
                            return;
                        }

                        // Recreate the texture only when the viewport size
                        // changed; otherwise keep the existing GPU resource
                        // and just update its contents in place.
                        let needs_new = match state.surface_textures.get(&webview_id) {
                            Some(surface_tex) => {
                                surface_tex.width() != width || surface_tex.height() != height
                            }
                            None => true,
                        };
                        if needs_new {
                            if let Some(old) = state.surface_textures.remove(&webview_id)
                                && old.is_registered()
                            {
                                state.renderer.unregister_resource(old.resource_id());
                            }
                            let texture =
                                Self::create_surface_texture(&device_handle.device, width, height);
                            let mut surface_tex = WebviewSurfaceTexture::CpuUpload {
                                texture,
                                resource_id: ResourceId::new(),
                                width,
                                height,
                                registered: false,
                            };
                            Self::register_surface_texture(&mut state.renderer, &mut surface_tex);
                            state.surface_textures.insert(webview_id, surface_tex);
                        }

                        // Upload the new pixels in place into the persistent
                        // texture. `write_texture` copies the shared-memory
                        // bytes into a staging buffer synchronously; no
                        // allocation or Vello re-registration is involved.
                        if let Some(surface_tex) = state.surface_textures.get_mut(&webview_id) {
                            device_handle.queue.write_texture(
                                wgpu::TexelCopyTextureInfo {
                                    texture: surface_tex.texture(),
                                    mip_level: 0,
                                    origin: wgpu::Origin3d::ZERO,
                                    aspect: wgpu::TextureAspect::All,
                                },
                                &pixels[..total_pixels],
                                wgpu::TexelCopyBufferLayout {
                                    offset: 0,
                                    bytes_per_row: Some(width * 4),
                                    rows_per_image: Some(height),
                                },
                                wgpu::Extent3d {
                                    width,
                                    height,
                                    depth_or_array_layers: 1,
                                },
                            );
                        } else {
                            error!(
                                "[embedder] missing surface texture for webview={:?} after insert",
                                webview_id
                            );
                        }
                    }
                    #[cfg(target_os = "macos")]
                    SurfaceFrame::SharedTexture {
                        texture_id,
                        surface_id,
                        port,
                    } => {
                        // Zero-copy path: the frame was rendered directly
                        // into a shared IOSurface by the graphics process.
                        // Import the surface (once per texture id) as the
                        // webview's persistent texture and blit it.
                        info!(
                            "[render-pipe] Embedder shared surface webview={:?} {}x{} texture_id={} active_tab={:?}",
                            webview_id, width, height, texture_id, state.active_tab
                        );
                        let is_new_surface = match state.surface_textures.get(&webview_id) {
                            Some(surface_tex) => {
                                surface_tex.width() != width
                                    || surface_tex.height() != height
                                    || surface_tex.texture_id() != Some(texture_id)
                            }
                            None => true,
                        };
                        if is_new_surface {
                            if let Some(old) = state.surface_textures.remove(&webview_id)
                                && old.is_registered()
                            {
                                state.renderer.unregister_resource(old.resource_id());
                            }
                            // Look the shared surface up by its global ID
                            // first; fall back to the Mach port when the ID
                            // is not resolvable.
                            let surface_ref = objc2_io_surface::IOSurfaceRef::lookup(surface_id)
                                .or_else(|| {
                                    let port_name = port.into_name();
                                    let surface =
                                        objc2_io_surface::IOSurfaceRef::lookup_from_mach_port(
                                            port_name,
                                        );
                                    deallocate_mach_port(port_name);
                                    surface
                                });
                            let Some(surface_ref) = surface_ref else {
                                error!(
                                    "[embedder] IOSurfaceLookup failed for webview={:?} id={surface_id}",
                                    webview_id
                                );
                                return;
                            };
                            let padded_width = Self::padded_surface_width(width);
                            let Some(texture) = Self::import_shared_surface(
                                &device_handle.device,
                                &surface_ref,
                                padded_width,
                                height,
                            ) else {
                                error!(
                                    "[embedder] failed to import shared surface for webview={:?}",
                                    webview_id
                                );
                                return;
                            };
                            let mut surface_tex = WebviewSurfaceTexture::SharedSurface {
                                texture,
                                resource_id: ResourceId::new(),
                                width,
                                height,
                                texture_width: padded_width,
                                registered: false,
                                texture_id,
                            };
                            Self::register_surface_texture(&mut state.renderer, &mut surface_tex);
                            state.surface_textures.insert(webview_id, surface_tex);
                        }
                    }
                }
                let is_active = state.active_tab == Some(webview_id);
                info!(
                    "[render-pipe] Embedder updated surface webview={:?} active={} tabs={:?} active_tab={:?} window={:?}",
                    webview_id, is_active, state.tab_order, state.active_tab, window_id
                );
                // A new surface frame is available; request a redraw so
                // the next paint blits it.
                Self::request_window_redraw(state);
                info!(
                    "[render-pipe] Embedder requested redraw for window={:?}",
                    window_id
                );
            }
            FormalWebUserEvent::Exit => event_loop.exit(),
        }
    }
}

// ── Chrome helpers ─────────────────────────────────────────────────────────

impl WindowedApp {
    #[allow(dead_code)]
    fn is_chrome_focused(windows: &HashMap<WindowId, WindowState>, window_id: WindowId) -> bool {
        windows
            .get(&window_id)
            .and_then(|state| state.chrome.as_ref())
            .is_some_and(ChromeUi::takes_text_input_focus)
    }

    fn pointer_in_chrome_st(
        windows: &HashMap<WindowId, WindowState>,
        window_id: WindowId,
        pos: PhysicalPosition<f64>,
    ) -> bool {
        windows
            .get(&window_id)
            .is_some_and(|state| Self::pointer_in_chrome(state, pos))
    }

    fn pointer_in_content_st(
        windows: &HashMap<WindowId, WindowState>,
        window_id: WindowId,
        pos: PhysicalPosition<f64>,
    ) -> bool {
        windows
            .get(&window_id)
            .is_some_and(|state| Self::pointer_in_content(state, pos))
    }

    fn map_button(mouse_button: MouseButton) -> MouseEventButton {
        match mouse_button {
            MouseButton::Left => MouseEventButton::Main,
            MouseButton::Right => MouseEventButton::Secondary,
            MouseButton::Middle => MouseEventButton::Auxiliary,
            MouseButton::Back => MouseEventButton::Fourth,
            MouseButton::Forward => MouseEventButton::Fifth,
            MouseButton::Other(_) => MouseEventButton::Auxiliary,
        }
    }

    fn chrome_event(app: &mut Self, window_id: WindowId, event: UiEvent) {
        let Some(state) = app.windows.get_mut(&window_id) else {
            return;
        };
        if !Self::has_visible_viewport(state) {
            return;
        }
        let action = state
            .chrome
            .as_mut()
            .and_then(|chrome| chrome.handle_ui_event(event));
        // action consumed below if present
        Self::request_window_redraw(state);
        if let Some(action) = action {
            Self::handle_chrome_action(app, window_id, action);
        }
    }

    fn handle_chrome_action(app: &mut Self, window_id: WindowId, action: ChromeAction) {
        match action {
            ChromeAction::Navigate => {
                let url = app.windows.get_mut(&window_id).and_then(|state| {
                    state
                        .chrome
                        .as_ref()
                        .and_then(|chrome| normalize_browser_destination(&chrome.address_value()))
                });
                let Some(url) = url else {
                    return;
                };
                if let Some(state) = app.windows.get_mut(&window_id) {
                    if let Some(provider) = app.provider.as_ref()
                        && let Some(webview_id) = state.active_tab
                    {
                        let _ = provider.navigate(Some(webview_id), &url);
                        if let Some(tab) = state.tabs.get_mut(&webview_id) {
                            tab.pending_url = Some(url.clone());
                        }
                    }
                    Self::sync_chrome(state);
                    Self::request_window_redraw(state);
                }
            }
            ChromeAction::NewTab => {
                if let Some(provider) = app.provider.as_ref() {
                    let _ = provider.navigate(None, "about:blank");
                }
                if let Some(state) = app.windows.get_mut(&window_id) {
                    Self::sync_chrome(state);
                    Self::request_window_redraw(state);
                }
            }
            ChromeAction::NewWindow => {
                let _ = send_user_event(FormalWebUserEvent::CreateWindow);
            }
            ChromeAction::SwitchTab(index) => {
                if let Some(state) = app.windows.get_mut(&window_id) {
                    if let Some(&webview_id) = state.tab_order.get(index) {
                        state.active_tab = Some(webview_id);
                        Self::update_provider_viewport(state, &mut app.provider);
                    }
                    Self::sync_chrome(state);
                    Self::request_window_redraw(state);
                }
            }
        }
    }
}

// ── AutomationHost ─────────────────────────────────────────────────────────

impl AutomationHost for WindowedApp {
    fn automation_snapshot(&mut self) -> AutomationSnapshot {
        self.active_window_id
            .and_then(|id| self.windows.get(&id))
            .map(Self::auto_snapshot)
            .unwrap_or(AutomationSnapshot {
                webview_id: None,
                current_url: None,
                displayed_url: String::new(),
                navigable_id: None,
                has_top_level_traversable: false,
            })
    }

    fn automation_visible_frame_viewports(
        &mut self,
    ) -> Result<Vec<AutomationVisibleFrameViewport>, String> {
        Ok(Vec::new())
    }

    fn automation_screenshot(&mut self) -> Result<Vec<u8>, String> {
        automation_screenshot_png()
    }

    fn begin_automation_navigation(&mut self, url: String) -> Result<(), String> {
        let (_window_id, state) = match self
            .active_window_id
            .and_then(|id| self.windows.get_mut(&id).map(|state| (id, state)))
        {
            Some((window_id, state)) => (window_id, state),
            None => return Err(String::from("no window")),
        };
        let provider = self
            .provider
            .as_ref()
            .ok_or_else(|| String::from("no provider"))?;
        Self::navigate_active_tab(provider, state, &url)
    }

    fn automation_click(&mut self, x: f32, y: f32) -> Result<(), String> {
        let window_id = self
            .active_window_id
            .ok_or_else(|| String::from("no window"))?;
        let state = self
            .windows
            .get_mut(&window_id)
            .ok_or_else(|| String::from("no state"))?;
        Self::auto_click(state, &mut self.provider, x, y)
    }

    fn automation_click_element(&mut self, selector: String) -> Result<(), String> {
        let webview_id = self
            .active_window_id
            .and_then(|id| self.windows.get(&id))
            .and_then(|state| state.active_tab)
            .ok_or_else(|| String::from("no tab"))?;
        self.with_provider(|provider| {
            provider.click_element(webview_id, selector).ok();
        })
        .ok_or_else(|| String::from("no provider"))
    }

    fn automation_scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Result<(), String> {
        let window_id = self
            .active_window_id
            .ok_or_else(|| String::from("no window"))?;
        let state = self
            .windows
            .get_mut(&window_id)
            .ok_or_else(|| String::from("no state"))?;
        Self::auto_scroll(state, &mut self.provider, x, y, dx, dy)
    }

    fn automation_evaluate_script(
        &mut self,
        source: String,
        timeout: Duration,
    ) -> Result<Value, String> {
        let webview_id = self
            .active_window_id
            .and_then(|id| self.windows.get(&id))
            .and_then(|state| state.active_tab)
            .ok_or_else(|| String::from("no tab"))?;
        self.provider
            .as_ref()
            .ok_or_else(|| String::from("no provider"))?
            .evaluate_script(webview_id, source, timeout)
    }
}

// ── Automation and navigation helpers ─────────────────────────────────────

impl WindowedApp {
    fn navigate_active_tab(
        provider: &WebviewProvider,
        state: &mut WindowState,
        url: &str,
    ) -> Result<(), String> {
        let webview_id = state
            .active_tab
            .ok_or_else(|| String::from("no active tab"))?;
        provider.navigate(Some(webview_id), url)?;
        if let Some(tab) = state.tabs.get_mut(&webview_id) {
            tab.pending_url = Some(url.to_owned());
        }
        Ok(())
    }

    fn auto_click(
        state: &mut WindowState,
        provider: &mut Option<WebviewProvider>,
        x: f32,
        y: f32,
    ) -> Result<(), String> {
        let Some(window) = state.window.as_ref() else {
            return Err(String::from("no window"));
        };
        let scale = window.scale_factor();
        let chrome_height = f64::from(Self::chrome_height_css(state));
        let pos =
            PhysicalPosition::new(f64::from(x) * scale, (f64::from(y) + chrome_height) * scale);
        state.pointer_pos = pos;
        if let Some(chrome) = state.chrome.as_mut() {
            chrome.clear_focus();
        }
        Self::request_window_redraw(state);
        let modifiers = winit_modifiers_to_kbt_modifiers(state.keyboard_modifiers.state());
        let coords = Self::ct_coords(state, pos);
        let send_event = |provider: &mut WebviewProvider, webview_id: WebviewId, ui_event| {
            provider.send_ui_event(webview_id, ui_event).ok();
        };
        let make_pointer_event = |mouse_button: MouseEventButton,
                                  mouse_buttons: MouseEventButtons|
         -> BlitzPointerEvent {
            BlitzPointerEvent {
                id: BlitzPointerId::Mouse,
                is_primary: true,
                coords,
                button: mouse_button,
                buttons: mouse_buttons,
                mods: modifiers,
                details: PointerDetails::default(),
            }
        };
        let Some(webview_id) = state.active_tab else {
            return Ok(());
        };
        if let Some(provider) = provider.as_mut() {
            send_event(
                provider,
                webview_id,
                UiEvent::PointerMove(make_pointer_event(Default::default(), state.buttons)),
            );
        }
        state.buttons |= MouseEventButton::Main.into();
        if let Some(provider) = provider.as_mut() {
            send_event(
                provider,
                webview_id,
                UiEvent::PointerDown(make_pointer_event(MouseEventButton::Main, state.buttons)),
            );
        }
        state.buttons.remove(MouseEventButton::Main.into());
        if let Some(provider) = provider.as_mut() {
            send_event(
                provider,
                webview_id,
                UiEvent::PointerUp(make_pointer_event(MouseEventButton::Main, state.buttons)),
            );
        }
        Ok(())
    }

    fn auto_scroll(
        state: &mut WindowState,
        provider: &mut Option<WebviewProvider>,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    ) -> Result<(), String> {
        let Some(window) = state.window.as_ref() else {
            return Err(String::from("no window"));
        };
        let scale = window.scale_factor();
        let chrome_height = f64::from(Self::chrome_height_css(state));
        let pos =
            PhysicalPosition::new(f64::from(x) * scale, (f64::from(y) + chrome_height) * scale);
        state.pointer_pos = pos;
        let modifiers = winit_modifiers_to_kbt_modifiers(state.keyboard_modifiers.state());
        let coords = Self::ct_coords(state, pos);
        let pointer_move_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords,
            button: Default::default(),
            buttons: state.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        let Some(webview_id) = state.active_tab else {
            return Ok(());
        };
        if let Some(provider) = provider.as_mut() {
            let _ = provider.send_ui_event(webview_id, UiEvent::PointerMove(pointer_move_event));
        }
        if let Some(provider) = provider.as_mut() {
            let _ = provider.send_ui_event(
                webview_id,
                UiEvent::Wheel(BlitzWheelEvent {
                    delta: BlitzWheelDelta::Pixels(f64::from(dx), f64::from(dy)),
                    coords,
                    buttons: state.buttons,
                    mods: modifiers,
                }),
            );
        }
        Ok(())
    }
}
