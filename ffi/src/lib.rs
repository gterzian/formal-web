use anyrender::WindowRenderer;
use anyrender_vello::VelloWindowRenderer;
use blitz_dom::BaseDocument;
use blitz_dom::DocumentConfig;
use blitz_paint::paint_scene;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, ShellProvider, Viewport};
use blitz_html::HtmlDocument;
use data_url::DataUrl;
use std::ffi::{CStr, c_char};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

#[repr(C)]
pub struct lean_object {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn lean_mk_string_from_bytes(value: *const c_char, size: usize) -> *mut lean_object;
    fn formal_web_handle_runtime_message(message: *mut lean_object) -> *mut lean_object;
    fn formal_web_user_agent_note_rendering_opportunity(message: *mut lean_object) -> *mut lean_object;
    fn formal_web_lean_io_result_mk_ok_unit() -> *mut lean_object;
    fn formal_web_lean_io_result_mk_error_from_bytes(
        value: *const c_char,
        size: usize,
    ) -> *mut lean_object;
    fn formal_web_lean_io_result_is_ok(result: *mut lean_object) -> u8;
    fn formal_web_lean_io_result_show_error(result: *mut lean_object);
    fn formal_web_lean_string_cstr(value: *mut lean_object) -> *const c_char;
    fn formal_web_lean_dec(value: *mut lean_object);
}

const EMPTY_HTML_DOCUMENT: &str = "<html><head></head><body></body></html>";
const LOADED_HTML_DOCUMENT: &str =
    "<!DOCTYPE html><html><head><style type=\"text/css\">html, body { height: 100%; margin: 0; } body { display: grid; place-items: center; background: #f4e8d2; }</style></head><body><svg width=\"368\" height=\"106\" viewBox=\"0 0 368 106\" version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\" style=\"display:block;fill-rule:evenodd;clip-rule:evenodd;stroke-linejoin:round;stroke-miterlimit:2;\"><g><path d=\"M131.548,97.488L131.548,8.369L144.939,8.369C150.903,8.369 155.656,8.831 159.196,9.755C162.774,10.678 165.795,12.236 168.258,14.43C170.759,16.7 172.741,19.528 174.203,22.915C175.703,26.339 176.454,29.802 176.454,33.304C176.454,39.692 174.01,45.098 169.123,49.523C173.856,51.139 177.589,53.967 180.321,58.008C183.091,62.01 184.477,66.666 184.477,71.976C184.477,78.941 182.014,84.828 177.089,89.638C174.126,92.601 170.797,94.66 167.103,95.814C163.063,96.93 158.003,97.488 151.923,97.488L131.548,97.488ZM144.997,46.637L149.21,46.637C154.213,46.637 157.878,45.531 160.206,43.318C162.534,41.106 163.698,37.845 163.698,33.535C163.698,29.341 162.505,26.156 160.119,23.982C157.734,21.808 154.27,20.721 149.73,20.721L144.997,20.721L144.997,46.637ZM144.997,84.847L153.308,84.847C159.388,84.847 163.852,83.654 166.699,81.269C169.701,78.691 171.201,75.42 171.201,71.456C171.201,67.608 169.758,64.376 166.872,61.76C164.063,59.181 159.042,57.892 151.808,57.892L144.997,57.892L144.997,84.847Z\" style=\"fill-rule:nonzero;\"/><rect x=\"202.173\" y=\"0\" width=\"12.987\" height=\"97.488\" style=\"fill-rule:nonzero;\"/><path d=\"M247.806,41.269L247.806,97.488L234.819,97.488L234.819,41.269L247.806,41.269ZM232.857,17.893C232.857,15.623 233.684,13.66 235.338,12.006C236.993,10.351 238.975,9.524 241.284,9.524C243.631,9.524 245.632,10.351 247.286,12.006C248.941,13.622 249.768,15.603 249.768,17.951C249.768,20.298 248.941,22.299 247.286,23.953C245.67,25.608 243.689,26.435 241.341,26.435C238.994,26.435 236.993,25.608 235.338,23.953C233.684,22.299 232.857,20.279 232.857,17.893Z\" style=\"fill-rule:nonzero;\"/><path d=\"M285.856,53.39L285.856,97.488L272.869,97.488L272.869,53.39L267.328,53.39L267.328,41.269L272.869,41.269L272.869,20.663L285.856,20.663L285.856,41.269L295.957,41.269L295.957,53.39L285.856,53.39Z\" style=\"fill-rule:nonzero;\"/><path d=\"M331.64,85.251L365.059,85.251L365.059,97.488L305.897,97.488L342.318,53.39L313.631,53.39L313.631,41.269L368.003,41.269L331.64,85.251Z\" style=\"fill-rule:nonzero;\"/></g><g><g><circle cx=\"53\" cy=\"53\" r=\"53\" style=\"fill:rgb(1,99,63);\"/><circle cx=\"53\" cy=\"53\" r=\"45.773\" style=\"fill:rgb(0,118,114);\"/><circle cx=\"53\" cy=\"53\" r=\"38.545\" style=\"fill:rgb(62,149,147);\"/><circle cx=\"53\" cy=\"53\" r=\"31.318\" style=\"fill:rgb(252,176,64);\"/><circle cx=\"53\" cy=\"53\" r=\"24.091\" style=\"fill:rgb(233,86,41);\"/><circle cx=\"53\" cy=\"53\" r=\"16.864\" style=\"fill:rgb(230,29,50);\"/></g><g><path d=\"M39.759,90.287C39.549,90.287 39.338,90.241 39.137,90.144C38.49,89.83 38.177,89.087 38.404,88.405L49.211,55.986L38.33,55.986C37.853,55.986 37.407,55.747 37.141,55.35C36.875,54.953 36.826,54.448 37.011,54.008L51.303,19.707C51.524,19.174 52.045,18.826 52.622,18.826L66.2,18.826C66.684,18.826 67.136,19.072 67.399,19.478C67.663,19.886 67.702,20.397 67.504,20.839L56.257,45.982L66.914,45.982C67.439,45.982 67.922,46.27 68.172,46.73C68.422,47.192 68.398,47.754 68.11,48.193L40.955,89.64C40.682,90.057 40.228,90.287 39.759,90.287Z\" style=\"fill:rgb(244,232,210);fill-rule:nonzero;\"/></g></g></svg></body></html>";

static EVENT_LOOP_PROXY: LazyLock<Mutex<Option<EventLoopProxy<FormalWebUserEvent>>>> =
    LazyLock::new(|| Mutex::new(None));
static WINDOW_VIEWPORT_SNAPSHOT: LazyLock<Mutex<Option<(u32, u32, f32, ColorScheme)>>> =
    LazyLock::new(|| Mutex::new(None));

enum FormalWebUserEvent {
    Paint(usize),
    DocumentRequestRedraw,
    RuntimeMessage(String),
}

struct DataOnlyNetProvider;

struct FormalWebShellProvider;

impl ShellProvider for FormalWebShellProvider {
    fn request_redraw(&self) {
        with_event_loop_proxy(|proxy| {
            if let Some(proxy) = proxy {
                let _ = proxy.send_event(FormalWebUserEvent::DocumentRequestRedraw);
            }
        });
    }
}

impl NetProvider for DataOnlyNetProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        match request.url.scheme() {
            "data" => match DataUrl::process(request.url.as_str()) {
                Ok(data_url) => match data_url.decode_to_vec() {
                    Ok((bytes, _fragment)) => handler.bytes(request.url.to_string(), Bytes::from(bytes)),
                    Err(_error) => {}
                },
                Err(_error) => {}
            },
            _scheme => {}
        }
    }
}

fn create_html_document_pointer(html: &str) -> usize {
    let viewport = WINDOW_VIEWPORT_SNAPSHOT
        .lock()
        .expect("window viewport snapshot mutex poisoned")
        .as_ref()
        .map(|(width, height, scale, color_scheme)| {
            Viewport::new(*width, *height, *scale, *color_scheme)
        });
    let document = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport,
            net_provider: Some(Arc::new(DataOnlyNetProvider)),
            shell_provider: Some(Arc::new(FormalWebShellProvider)),
            ..DocumentConfig::default()
        },
    );
    Box::into_raw(Box::new(document)) as usize
}

fn lean_string_from_owned(value: String) -> *mut lean_object {
    unsafe { lean_mk_string_from_bytes(value.as_ptr() as *const c_char, value.len()) }
}

fn ok_unit_result() -> *mut lean_object {
    unsafe { formal_web_lean_io_result_mk_ok_unit() }
}

fn error_result(message: &str) -> *mut lean_object {
    unsafe { formal_web_lean_io_result_mk_error_from_bytes(message.as_ptr() as *const c_char, message.len()) }
}

fn call_lean_runtime_message_handler(message: &str) {
    let lean_message = lean_string_from_owned(message.to_owned());
    let io_result = unsafe { formal_web_handle_runtime_message(lean_message) };

    let is_ok = unsafe { formal_web_lean_io_result_is_ok(io_result) } != 0;
    if !is_ok {
        unsafe { formal_web_lean_io_result_show_error(io_result) };
    }

    unsafe { formal_web_lean_dec(io_result) };
}

fn user_agent_note_rendering_opportunity(message: &str) {
    let lean_message = lean_string_from_owned(message.to_owned());
    let io_result = unsafe { formal_web_user_agent_note_rendering_opportunity(lean_message) };

    let is_ok = unsafe { formal_web_lean_io_result_is_ok(io_result) } != 0;
    if !is_ok {
        unsafe { formal_web_lean_io_result_show_error(io_result) };
    }

    unsafe { formal_web_lean_dec(io_result) };
}

fn with_event_loop_proxy<R>(f: impl FnOnce(&Option<EventLoopProxy<FormalWebUserEvent>>) -> R) -> R {
    let guard = EVENT_LOOP_PROXY
        .lock()
        .expect("event loop proxy mutex poisoned");
    f(&guard)
}

fn queue_paint(pointer: usize) -> Result<(), String> {
    with_event_loop_proxy(|proxy| match proxy {
        Some(proxy) => proxy
            .send_event(FormalWebUserEvent::Paint(pointer))
            .map_err(|error| format!("failed to queue paint event: {error}")),
        None => Err(String::from("winit event loop proxy is not initialized")),
    })
}

struct FormalWebApp {
    window: Option<Arc<Window>>,
    renderer: VelloWindowRenderer,
    pending_base_document: Option<usize>,
    saw_redraw_requested: bool,
    has_top_level_traversable: bool,
    animation_timer: Option<Instant>,
}

impl Default for FormalWebApp {
    fn default() -> Self {
        Self {
            window: None,
            renderer: VelloWindowRenderer::new(),
            pending_base_document: None,
            saw_redraw_requested: false,
            has_top_level_traversable: false,
            animation_timer: None,
        }
    }
}

impl FormalWebApp {
    fn update_window_viewport_snapshot(window: &Window) {
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let color_scheme = match window.theme().unwrap_or(winit::window::Theme::Light) {
            winit::window::Theme::Light => ColorScheme::Light,
            winit::window::Theme::Dark => ColorScheme::Dark,
        };
        let mut snapshot = WINDOW_VIEWPORT_SNAPSHOT
            .lock()
            .expect("window viewport snapshot mutex poisoned");
        *snapshot = Some((size.width, size.height, scale, color_scheme));
    }

    fn resume_renderer_for_window(&mut self, window: &Arc<Window>) {
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        if self.renderer.is_active() {
            self.renderer.set_size(size.width, size.height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            self.renderer.resume(window_handle, size.width, size.height);
        }
    }

    fn current_animation_time(&mut self) -> f64 {
        match self.animation_timer {
            Some(start) => Instant::now().duration_since(start).as_secs_f64(),
            None => {
                self.animation_timer = Some(Instant::now());
                0.0
            }
        }
    }

    fn create_window(event_loop: &ActiveEventLoop) -> Result<Arc<Window>, String> {
        let attributes: WindowAttributes = Window::default_attributes()
            .with_title("formal-web winit demo");
        event_loop
            .create_window(attributes)
            .map(Arc::new)
            .map_err(|error| format!("failed to create winit window: {error}"))
    }

    fn paint_base_document(&mut self, pointer: usize) {
        if pointer == 0 {
            return;
        }

        let animation_time = self.current_animation_time();
        let Some(window) = self.window.as_ref() else {
            return;
        };

        let base_document = unsafe { &mut *(pointer as *mut BaseDocument) };
        let size = window.inner_size();
        let scale_factor = window.scale_factor() as f32;
        base_document.set_viewport(Viewport::new(
            size.width,
            size.height,
            scale_factor,
            ColorScheme::Light,
        ));
        for pass in 0..3 {
            base_document.resolve(animation_time);
            let _ = pass;
            if !base_document.has_pending_critical_resources() {
                break;
            }
        }

        let (width, height) = base_document.viewport().window_size;
        let scale = base_document.viewport().scale_f64();

        if self.renderer.is_active() {
            self.renderer.set_size(width, height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            self.renderer.resume(window_handle, width, height);
        }

        self.renderer.render(|scene| {
            paint_scene(scene, &*base_document, scale, width, height, 0, 0)
        });
    }
}

impl ApplicationHandler<FormalWebUserEvent> for FormalWebApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            match Self::create_window(event_loop) {
                Ok(window) => {
                    Self::update_window_viewport_snapshot(&window);
                    self.resume_renderer_for_window(&window);
                    self.window = Some(window);
                    call_lean_runtime_message_handler("FreshTopLevelTraversable");
                }
                Err(error) => {
                    let _ = error;
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::RedrawRequested => {
                self.saw_redraw_requested = true;
                if let Some(pointer) = self.pending_base_document.take() {
                    self.paint_base_document(pointer);
                    self.saw_redraw_requested = false;
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(window) = self.window.as_ref() {
                    Self::update_window_viewport_snapshot(window);
                }
                if self.renderer.is_active() {
                    self.renderer.set_size(size.width, size.height);
                }
                if self.has_top_level_traversable {
                    window.request_redraw();
                    user_agent_note_rendering_opportunity("request_redraw");
                }
            }
            WindowEvent::CloseRequested => {
                self.renderer.suspend();
                self.animation_timer = None;
                self.has_top_level_traversable = false;
                if let Ok(mut snapshot) = WINDOW_VIEWPORT_SNAPSHOT.lock() {
                    *snapshot = None;
                }
                self.window = None;
                event_loop.exit();
            }
            WindowEvent::Destroyed => {
                self.renderer.suspend();
                self.animation_timer = None;
                self.has_top_level_traversable = false;
                if let Ok(mut snapshot) = WINDOW_VIEWPORT_SNAPSHOT.lock() {
                    *snapshot = None;
                }
                self.window = None;
                event_loop.exit();
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: FormalWebUserEvent) {
        match event {
            FormalWebUserEvent::Paint(pointer) => {
                let Some(_window) = self.window.as_ref() else {
                    return;
                };

                if self.saw_redraw_requested {
                    self.paint_base_document(pointer);
                    self.saw_redraw_requested = false;
                } else {
                    self.pending_base_document = Some(pointer);
                }
            }
            FormalWebUserEvent::DocumentRequestRedraw => {
                if self.has_top_level_traversable {
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                        user_agent_note_rendering_opportunity("request_redraw");
                    }
                }
            }
            FormalWebUserEvent::RuntimeMessage(message) => {
                match message.as_str() {
                    "NewTopLevelTraversable" => {
                        self.has_top_level_traversable = true;
                        if let Some(window) = self.window.as_ref() {
                            window.request_redraw();
                            user_agent_note_rendering_opportunity("request_redraw");
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_create_empty_html_document(_: *mut lean_object) -> usize {
    panic::catch_unwind(AssertUnwindSafe(|| create_html_document_pointer(EMPTY_HTML_DOCUMENT)))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_create_loaded_html_document(_: *mut lean_object) -> usize {
    panic::catch_unwind(AssertUnwindSafe(|| create_html_document_pointer(LOADED_HTML_DOCUMENT)))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_render_html_document(pointer: usize) -> *mut lean_object {
    let html = panic::catch_unwind(AssertUnwindSafe(|| {
        if pointer == 0 {
            String::from("<null rust document pointer>")
        } else {
            let document = unsafe { &*(pointer as *const HtmlDocument) };
            document.root_element().outer_html()
        }
    }))
    .unwrap_or_else(|_| String::from("<panic rendering rust document>"));

    lean_string_from_owned(html)
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_extract_base_document(pointer: usize) -> usize {
    panic::catch_unwind(AssertUnwindSafe(|| {
        if pointer == 0 {
            0
        } else {
            let document = unsafe { &*(pointer as *const HtmlDocument) };
            let base_document: *const BaseDocument = &**document;
            base_document as usize
        }
    }))
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_queue_paint(pointer: usize, _: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| queue_paint(pointer))) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic queueing paint event"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_send_runtime_message(message: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_message = unsafe { formal_web_lean_string_cstr(message) };
        let message = unsafe { CStr::from_ptr(c_message) }
            .to_string_lossy()
            .into_owned();
        with_event_loop_proxy(|proxy| match proxy {
            Some(proxy) => proxy
                .send_event(FormalWebUserEvent::RuntimeMessage(message))
                .map_err(|error| format!("failed to send runtime message event: {error}")),
            None => Err(String::from("winit event loop proxy is not initialized")),
        })
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic sending runtime message"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_run_winit_event_loop(_: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let event_loop = EventLoop::<FormalWebUserEvent>::with_user_event()
            .build()
            .map_err(|error| format!("failed to create event loop: {error}"))?;
        {
            let mut guard = EVENT_LOOP_PROXY
                .lock()
                .expect("event loop proxy mutex poisoned");
            *guard = Some(event_loop.create_proxy());
        }
        let mut app = FormalWebApp::default();
        let run_result = event_loop
            .run_app(&mut app)
            .map_err(|error| format!("winit event loop failed: {error}"));

        {
            let mut guard = EVENT_LOOP_PROXY
                .lock()
                .expect("event loop proxy mutex poisoned");
            guard.take();
        }

        run_result
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic running winit event loop"),
    }
}