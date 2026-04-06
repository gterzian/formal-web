use std::collections::VecDeque;

use ipc_messages::content::CallbackData;

use super::execution_context::JsExecutionContext;

type TaskCallback = Box<dyn FnOnce(&mut JsExecutionContext) -> Result<(), String>>;
/// <https://html.spec.whatwg.org/#queue-a-task>
pub type PendingCallback = Box<
    dyn FnOnce(&mut JsExecutionContext, CallbackData) -> Result<(), String>,
>;

/// <https://html.spec.whatwg.org/#concept-task>
pub struct Task {
    callback: TaskCallback,
}

/// <https://html.spec.whatwg.org/#task-queue>
#[derive(Default)]
pub struct TaskQueue {
    tasks: VecDeque<Task>,
}

impl TaskQueue {
    pub fn push(
        &mut self,
        callback: impl FnOnce(&mut JsExecutionContext) -> Result<(), String> + 'static,
    ) {
        self.tasks.push_back(Task {
            callback: Box::new(callback),
        });
    }

    pub fn push_task(&mut self, task: Task) {
        self.tasks.push_back(task);
    }

    pub fn pop_front(&mut self) -> Option<Task> {
        self.tasks.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

impl Task {
    pub fn run(self, execution_context: &mut JsExecutionContext) -> Result<(), String> {
        (self.callback)(execution_context)
    }
}