use alloc::collections::VecDeque;

use protocol::Message;

#[derive(Debug, Clone)]
pub enum SessionEvent {
    Message(Message),
    TaskTimeout(u64),
}

pub struct EventQueue {
    inner: VecDeque<SessionEvent>,
}

impl EventQueue {
    pub fn new() -> Self {
        Self {
            inner: VecDeque::with_capacity(16),
        }
    }

    pub fn push(&mut self, event: SessionEvent) {
        self.inner.push_back(event)
    }

    pub fn pop(&mut self) -> Option<SessionEvent> {
        self.inner.pop_front()
    }
}
