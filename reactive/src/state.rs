use core::cell::RefCell;

use alloc::rc::{Rc, Weak};

use fnv::FnvBuildHasher;
use indexmap::IndexMap;

use super::effect::CONTEXTS;

pub(super) type CallbackPtr = *const RefCell<dyn FnMut()>;

pub(super) type Callback = Weak<RefCell<dyn FnMut()>>;

pub(super) struct Signal<T> {
    value: Rc<T>,
    emitter: IndexMap<CallbackPtr, Callback, FnvBuildHasher>,
}

pub(super) trait SignalEmitter {
    fn subscribe(&self, handler: Callback);
    fn unsubscribe(&self, handler: CallbackPtr);
}

impl<T> SignalEmitter for RefCell<Signal<T>> {
    fn subscribe(&self, handler: Callback) {
        self.borrow_mut()
            .emitter
            .insert(Weak::as_ptr(&handler), handler);
    }

    fn unsubscribe(&self, handler: CallbackPtr) {
        self.borrow_mut().emitter.swap_remove(&handler);
    }
}

#[derive(Clone)]
pub struct StateHandle<T>(Rc<RefCell<Signal<T>>>);

impl<T: 'static> StateHandle<T> {
    pub fn new(value: T) -> Self {
        Self(Rc::new(RefCell::new(Signal {
            value: Rc::new(value),
            emitter: IndexMap::default(),
        })))
    }

    #[inline]
    pub fn get(&self) -> Rc<T> {
        Rc::clone(&self.0.borrow().value)
    }

    pub fn get_tracked(&self) -> Rc<T> {
        self.track();
        self.get()
    }

    pub fn set(&self, value: T) {
        self.0.borrow_mut().value = Rc::new(value);
        self.notify();
    }

    pub fn track(&self) {
        CONTEXTS.with(|effects| {
            if let Some(last) = effects.borrow().last() {
                let signal = Rc::clone(&self.0);

                last.upgrade()
                    .expect("Running should be valid while inside reactive scope")
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .add_dependency(signal);
            }
        });
    }

    pub fn notify(&self) {
        let subscribers = self.0.borrow().emitter.clone();
        for subscriber in subscribers.values().rev() {
            if let Some(callback) = subscriber.upgrade() {
                callback.borrow_mut()();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_state() {
        let state = StateHandle::new(0);

        assert_eq!(*state.get_tracked(), 0);

        state.set(1);
        assert_eq!(*state.get_tracked(), 1);
    }

    #[test]
    fn test_state_composition() {
        let state = StateHandle::new(0);
        let double = || *state.get_tracked() * 2;

        assert_eq!(double(), 0);

        state.set(1);
        assert_eq!(double(), 2);
    }
}
