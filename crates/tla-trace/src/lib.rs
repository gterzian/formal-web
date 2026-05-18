mod bootstrap;
mod monitor;
mod tracer;
mod types;

pub use bootstrap::{receive_monitor_sender, spawn_monitor_sender_bridge};
pub use monitor::Monitor;
pub use tracer::TLATracer;
pub use types::{LogEntry, VarUpdate};

#[macro_export]
macro_rules! tla_log {
    ($tracer:expr) => {{
        let tracer = &mut $tracer;
        if tracer.is_enabled() {
            tracer.log_silent_with_location(file!(), line!());
        }
    }};
    ($tracer:expr, $event:expr $(, $arg:expr)* $(,)?) => {{
        let tracer = &mut $tracer;
        if tracer.is_enabled() {
            tracer.log_with_location(
                $event,
                vec![$(::std::string::ToString::to_string(&$arg)),*],
                file!(),
                line!(),
            );
        }
    }};
}
