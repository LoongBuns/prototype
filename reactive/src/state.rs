use core::cell::RefCell;

use alloc::rc::Rc;

use fnv::FnvBuildHasher;
use indexmap::IndexMap;

use super::FiberValue;
use super::effect::{CONTEXTS, Callback, CallbackPtr, Dependency};

pub(super) struct SignalInner<T> {
    inner: Rc<T>,
    subscribers: IndexMap<CallbackPtr, Callback, FnvBuildHasher>,
}

impl<T> SignalInner<T> {
    fn new(value: T) -> Self {
        Self {
            inner: Rc::new(value),
            subscribers: IndexMap::default(),
        }
    }

    fn subscribe(&mut self, handler: Callback) {
        self.subscribers.insert(handler.as_ptr(), handler);
    }

    fn unsubscribe(&mut self, handler: CallbackPtr) {
        self.subscribers.swap_remove(&handler);
    }

    fn update(&mut self, new_value: T) {
        self.inner = Rc::new(new_value);
    }
}

pub(super) trait DynSignalInner {
    fn subscribe(&self, handler: Callback);
    fn unsubscribe(&self, handler: CallbackPtr);
}

impl<T> DynSignalInner for RefCell<SignalInner<T>> {
    fn subscribe(&self, handler: Callback) {
        self.borrow_mut().subscribe(handler);
    }

    fn unsubscribe(&self, handler: CallbackPtr) {
        self.borrow_mut().unsubscribe(handler);
    }
}

#[repr(C)]
pub struct StateHandle(*mut Rc<RefCell<SignalInner<FiberValue>>>);

#[unsafe(no_mangle)]
pub extern "C" fn use_state(value: FiberValue) -> *mut StateHandle {
    let ptr = Box::into_raw(Box::new(Rc::new(RefCell::new(SignalInner::new(value)))));
    Box::into_raw(Box::new(StateHandle(ptr)))
}

#[unsafe(no_mangle)]
pub extern "C" fn state_get(handle: *const StateHandle) -> FiberValue {
    if !handle.is_null() {
        let signal = unsafe { &*(&*(handle)).0 };

        CONTEXTS.with(|contexts| {
            if let Some(last_context) = contexts.borrow().last() {
                let signal = Rc::clone(signal);

                last_context
                    .upgrade()
                    .expect("Running should be valid while inside reactive scope")
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .dependencies
                    .insert(Dependency(signal));
            }
        });

        (*signal.borrow().inner).clone()
    } else {
        FiberValue::Void
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn state_get_raw(handle: *const StateHandle) -> FiberValue {
    if !handle.is_null() {
        let signal = unsafe { &*(&*(handle)).0 };
        (*signal.borrow().inner).clone()
    } else {
        FiberValue::Void
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn state_set(handle: *mut StateHandle, value: FiberValue) {
    let signal = unsafe { &*(&*(handle)).0 };
    signal.borrow_mut().update(value);

    let subscribers = signal.borrow().subscribers.clone();
    for subscriber in subscribers.values().rev() {
        if let Some(callback) = subscriber.callback() {
            callback.borrow_mut()();
        }
    }
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;

    use alloc::rc::Rc;

    use crate::*;

    #[test]
    fn test_state() {
        let state = use_state(FiberValue::I32(0));
        assert_eq!(state_get(state), FiberValue::I32(0));

        state_set(state, FiberValue::I32(1));
        assert_eq!(state_get(state), FiberValue::I32(1));
    }

    #[test]
    fn test_state_composition() {
        let state = use_state(FiberValue::I32(0));

        let double = || match state_get(state) {
            FiberValue::I32(v) => v * 2,
            _ => 0,
        };

        assert_eq!(double(), 0);

        state_set(state, FiberValue::I32(1));
        assert_eq!(double(), 2);
    }

    #[test]
    fn test_state_nested_effects() {
        let counter = Rc::new(Cell::new(0));
        let state1 = use_state(FiberValue::I32(0));
        let state2 = use_state(FiberValue::I32(0));

        create_effect({
            let counter_clone = Rc::clone(&counter);
            move || {
                if let FiberValue::I32(v) = state_get(state1) {
                    if v > 0 {
                        state_get(state2);
                    }
                }
                counter_clone.set(counter_clone.get() + 1);
            }
        });

        assert_eq!(counter.get(), 1);

        state_set(state2, FiberValue::I32(1));
        assert_eq!(counter.get(), 1);

        state_set(state1, FiberValue::I32(1));
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn test_state_cleanup() {
        let counter = Rc::new(Cell::new(0));
        let state = use_state(FiberValue::I32(0));
        
        create_effect({
            let counter_clone = Rc::clone(&counter);
            move || {
                state_get(state);
                let value = Rc::clone(&counter_clone);
                on_cleanup(move || {
                    value.set(value.get() + 1);
                });
            }
        });

        assert_eq!(counter.get(), 0);

        state_set(state, FiberValue::I32(1));
        assert_eq!(counter.get(), 1);

        state_set(state, FiberValue::I32(2));
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn test_state_complex_value() {
        let state = use_state(FiberValue::List(Box::into_raw(
            vec![FiberValue::I32(1), FiberValue::I32(2)].into_boxed_slice(),
        )));

        if let FiberValue::List(list) = state_get(state) {
            let list = unsafe { &*list };
            assert_eq!(list.len(), 2);
            assert_eq!(list[0], FiberValue::I32(1));
            assert_eq!(list[1], FiberValue::I32(2));
        } else {
            panic!("Expected list");
        }

        state_set(
            state,
            FiberValue::List(Box::into_raw(
                vec![FiberValue::I32(3), FiberValue::I32(4)].into_boxed_slice(),
            )),
        );

        if let FiberValue::List(list) = state_get(state) {
            let list = unsafe { &*list };
            assert_eq!(list.len(), 2);
            assert_eq!(list[0], FiberValue::I32(3));
            assert_eq!(list[1], FiberValue::I32(4));
        } else {
            panic!("Expected list");
        }
    }
}
