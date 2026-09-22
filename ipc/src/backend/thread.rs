//! In-process ("no IPC") backend.
//!
//! Extensions run on dedicated threads of the embedding process and exchange
//! messages over crossbeam channels, so no serialization happens and no
//! helper process is launched.  The embedding binary registers each
//! extension's entry point with [`register_extension_runner`]; `launch`
//! dispatches to the registered runner whose service name matches the
//! manifest's endpoint.
//!
//! An extension whose service has no registered runner is launched by the
//! compiled transport backend instead (ipc-channel or XPC), so the in-process
//! and process transports can be mixed in one build.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde::de::DeserializeOwned;

use crate::IpcError;
use crate::types::{
    ExtensionHandle, ExtensionHandleImpl, ExtensionServer, IpcChannelMessage, IpcConnection,
    IpcReceiver, IpcSender, IpcSerialize, IpcTransport,
};

pub(crate) type ExtensionRunFn<In, Out> =
    dyn Fn(ExtensionServer<In, Out>) -> Result<(), String> + Send + Sync + 'static;

pub(crate) struct ExtensionRunner<Out, In>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    run: Arc<ExtensionRunFn<In, Out>>,
}

type RunnerTable = Mutex<HashMap<&'static str, Arc<dyn Any + Send + Sync>>>;

fn runner_table() -> &'static RunnerTable {
    static RUNNERS: OnceLock<RunnerTable> = OnceLock::new();
    RUNNERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register the in-process entry point for the extension service named
/// `service_name` (the name its `ExtensionManifest::endpoint` reports).
pub fn register_extension_runner<Out, In>(
    service_name: &'static str,
    run: impl Fn(ExtensionServer<In, Out>) -> Result<(), String> + Send + Sync + 'static,
) where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let runner = ExtensionRunner::<Out, In> { run: Arc::new(run) };
    let mut table = match runner_table().lock() {
        Ok(table) => table,
        Err(poisoned) => poisoned.into_inner(),
    };
    table.insert(service_name, Arc::new(runner));
}

/// The runner registered for `service_name`, if any.
pub(crate) fn find_runner<Out, In>(service_name: &str) -> Option<Arc<ExtensionRunner<Out, In>>>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let table = match runner_table().lock() {
        Ok(table) => table,
        Err(poisoned) => poisoned.into_inner(),
    };
    let erased = table.get(service_name)?.clone();
    drop(table);
    erased.downcast::<ExtensionRunner<Out, In>>().ok()
}

/// Run `runner` on a new thread, connected to the returned connection.
pub(crate) fn launch<Out, In>(
    service_name: &'static str,
    runner: Arc<ExtensionRunner<Out, In>>,
) -> Result<(ExtensionHandle, IpcConnection<Out, In>), IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let (parent_sender, child_receiver) = crossbeam_channel::unbounded::<IpcChannelMessage<Out>>();
    let (child_sender, parent_receiver) = crossbeam_channel::unbounded::<IpcChannelMessage<In>>();

    let server = ExtensionServer::new(IpcConnection::new(
        IpcSender {
            transport: IpcTransport::Thread(child_sender),
        },
        IpcReceiver::from_thread_channel(child_receiver),
    ));

    let thread_name = format!("formal-web:{service_name}");
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            if let Err(error) = (runner.run)(server) {
                log::error!("in-process extension '{service_name}' failed: {error}");
            }
        })
        .map_err(|error| {
            IpcError::Transport(format!(
                "failed to spawn in-process extension thread: {error}"
            ))
        })?;

    let handle = ExtensionHandle {
        inner: ExtensionHandleImpl::Thread,
    };
    Ok((
        handle,
        IpcConnection::new(
            IpcSender {
                transport: IpcTransport::Thread(parent_sender),
            },
            IpcReceiver::from_thread_channel(parent_receiver),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ExtensionEndpoint, ExtensionManifest};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    enum TestRequest {
        Echo(String),
        Stop,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct TestResponse(String);

    struct TestManifest;

    impl ExtensionManifest for TestManifest {
        fn endpoint(&self) -> ExtensionEndpoint {
            ExtensionEndpoint::Singleton {
                service_name: "test.formal-web.thread",
            }
        }
    }

    fn test_runner(server: ExtensionServer<TestResponse, TestRequest>) -> Result<(), String> {
        loop {
            let incoming = server
                .connection
                .receiver
                .recv()
                .map_err(|error| format!("receive failed: {error}"))?;
            match incoming.payload {
                TestRequest::Echo(text) => server
                    .connection
                    .sender
                    .send(TestResponse(text))
                    .map_err(|error| format!("send failed: {error}"))?,
                TestRequest::Stop => return Ok(()),
            }
        }
    }

    #[test]
    fn in_process_round_trip() {
        register_extension_runner("test.formal-web.thread", test_runner);
        let (_handle, connection) =
            ExtensionHandle::launch::<TestManifest, TestRequest, TestResponse>(&TestManifest)
                .expect("in-process launch");

        connection
            .sender
            .send(TestRequest::Echo(String::from("hello")))
            .expect("send echo");
        let reply = connection.receiver.recv().expect("receive echo");
        let TestResponse(text) = reply.payload;
        assert_eq!(text, "hello");

        connection
            .sender
            .send(TestRequest::Stop)
            .expect("send stop");
    }
}
