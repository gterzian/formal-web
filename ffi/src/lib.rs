use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use std::ffi::{CStr, c_char};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::panic::{self, AssertUnwindSafe};
use std::thread;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

#[repr(C)]
pub struct lean_object {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn lean_mk_string_from_bytes(value: *const c_char, size: usize) -> *mut lean_object;
    fn formal_web_handle_winit_redraw(message: *mut lean_object) -> *mut lean_object;
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
    "<html><head><title>Loaded</title></head><body><p>Loaded!</p></body></html>";

static MESSAGE_SENDER: LazyLock<Mutex<Option<mpsc::Sender<String>>>> =
    LazyLock::new(|| Mutex::new(None));

fn create_html_document_pointer(html: &str) -> usize {
    let document = HtmlDocument::from_html(html, DocumentConfig::default());
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

fn with_message_sender<R>(f: impl FnOnce(&Option<mpsc::Sender<String>>) -> R) -> R {
    let guard = MESSAGE_SENDER
        .lock()
        .expect("runtime message sender mutex poisoned");
    f(&guard)
}

fn send_runtime_message(message: String) -> Result<(), String> {
    with_message_sender(|sender| match sender {
        Some(sender) => sender
            .send(message)
            .map_err(|error| format!("failed to send runtime message: {error}")),
        None => Err(String::from("runtime message channel is not initialized")),
    })
}

fn call_lean_redraw_handler(message: &str) {
    let lean_message = lean_string_from_owned(message.to_owned());
    let io_result = unsafe { formal_web_handle_winit_redraw(lean_message) };

    let is_ok = unsafe { formal_web_lean_io_result_is_ok(io_result) } != 0;
    if !is_ok {
        unsafe { formal_web_lean_io_result_show_error(io_result) };
    }

    unsafe { formal_web_lean_dec(io_result) };
}

#[derive(Default)]
struct FormalWebApp {
    window: Option<Arc<Window>>,
}

impl FormalWebApp {
    fn create_window(event_loop: &ActiveEventLoop) -> Result<Arc<Window>, String> {
        let attributes: WindowAttributes = Window::default_attributes()
            .with_title("formal-web winit demo");
        event_loop
            .create_window(attributes)
            .map(Arc::new)
            .map_err(|error| format!("failed to create winit window: {error}"))
    }
}

impl ApplicationHandler for FormalWebApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            match Self::create_window(event_loop) {
                Ok(window) => {
                    window.request_redraw();
                    self.window = Some(window);
                }
                Err(error) => {
                    eprintln!("{error}");
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
                call_lean_redraw_handler("redraw requested");
            }
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            _ => {}
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
pub extern "C" fn formal_web_send_runtime_message(message: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let c_message = unsafe { formal_web_lean_string_cstr(message) };
        let message = unsafe { CStr::from_ptr(c_message) }
            .to_string_lossy()
            .into_owned();
        send_runtime_message(message)
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic sending runtime message"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_run_winit_event_loop(_: *mut lean_object) -> *mut lean_object {
    match panic::catch_unwind(AssertUnwindSafe(|| {
        let (sender, receiver) = mpsc::channel::<String>();
        {
            let mut guard = MESSAGE_SENDER
                .lock()
                .expect("runtime message sender mutex poisoned");
            *guard = Some(sender);
        }

        let receiver_thread = thread::spawn(move || {
            while let Ok(message) = receiver.recv() {
                println!("runtime message: {message}");
            }
        });

        let event_loop = EventLoop::new().map_err(|error| format!("failed to create event loop: {error}"))?;
        let mut app = FormalWebApp::default();
        let run_result = event_loop
            .run_app(&mut app)
            .map_err(|error| format!("winit event loop failed: {error}"));

        {
            let mut guard = MESSAGE_SENDER
                .lock()
                .expect("runtime message sender mutex poisoned");
            guard.take();
        }

        receiver_thread
            .join()
            .map_err(|_| String::from("runtime message receiver thread panicked"))?;

        run_result
    })) {
        Ok(Ok(())) => ok_unit_result(),
        Ok(Err(error)) => error_result(&error),
        Err(_) => error_result("panic running winit event loop"),
    }
}