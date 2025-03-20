use core::cell::RefCell;
use core::ptr;

use alloc::rc::Rc;
use alloc::vec::Vec;

use super::FiberValue;
use super::effect::create_effect;
use super::state::{StateHandle, state_get, state_set, use_state};

type MapFn = extern "C" fn(usize, *const FiberValue) -> FiberValue;

#[unsafe(no_mangle)]
pub extern "C" fn map_indexed(list: *mut StateHandle, map_fn: MapFn) -> *mut StateHandle {
    if list.is_null() {
        return ptr::null_mut();
    }

    let result = use_state(FiberValue::List(Box::into_raw(
        Vec::new().into_boxed_slice(),
    )));
    let previous_items = Rc::new(RefCell::new(Vec::new()));
    let mapped_items = Rc::new(RefCell::new(Vec::new()));

    create_effect(move || {
        if let FiberValue::List(list_ptr) = state_get(list) {
            if list_ptr.is_null() {
                return;
            }

            let list = unsafe { &*list_ptr };
            let mut prev_items = previous_items.borrow_mut();
            let mut mapped = mapped_items.borrow_mut();

            if list.is_empty() {
                *prev_items = Vec::new();
                *mapped = Vec::new();
            } else {
                if list.len() > prev_items.len() {
                    mapped.reserve(list.len() - prev_items.len());
                }

                for (i, item) in list.iter().enumerate() {
                    if prev_items.get(i) != Some(item) {
                        let mapped_value = map_fn(i, item as *const FiberValue);
                        if let Some(existing) = mapped.get_mut(i) {
                            *existing = mapped_value;
                        } else {
                            mapped.push(mapped_value);
                        }
                    }
                }

                if list.len() < prev_items.len() {
                    mapped.truncate(list.len());
                }

                *prev_items = list.to_vec();
            }

            state_set(
                result,
                FiberValue::List(Box::into_raw(mapped.clone().into_boxed_slice())),
            );
        }
    });

    result
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;
    use core::ptr;

    use alloc::rc::Rc;

    use crate::*;

    extern "C" fn double_map(_: usize, x: *const FiberValue) -> FiberValue {
        if x.is_null() {
            return FiberValue::Void;
        }
        let x = unsafe { &*x };
        if let FiberValue::I32(v) = x {
            FiberValue::I32(v * 2)
        } else {
            FiberValue::Void
        }
    }

    #[test]
    fn test_map_indexed() {
        let list = use_state(FiberValue::List(Box::into_raw(
            vec![FiberValue::I32(1), FiberValue::I32(2), FiberValue::I32(3)].into_boxed_slice(),
        )));

        let mapped = map_indexed(list, double_map);

        if let FiberValue::List(result) = state_get(mapped) {
            let result = unsafe { &*result };
            assert_eq!(
                result,
                &[
                    FiberValue::I32(2), // 1 * 2
                    FiberValue::I32(4), // 2 * 2
                    FiberValue::I32(6), // 3 * 2
                ]
            );
        } else {
            panic!("Expected list");
        }

        state_set(
            list,
            FiberValue::List(Box::into_raw(
                vec![
                    FiberValue::I32(1),
                    FiberValue::I32(2),
                    FiberValue::I32(3),
                    FiberValue::I32(4),
                ]
                .into_boxed_slice(),
            )),
        );

        if let FiberValue::List(result) = state_get(mapped) {
            let result = unsafe { &*result };
            assert_eq!(
                result,
                &[
                    FiberValue::I32(2),
                    FiberValue::I32(4),
                    FiberValue::I32(6),
                    FiberValue::I32(8),
                ]
            );
        } else {
            panic!("Expected list");
        }

        state_set(
            list,
            FiberValue::List(Box::into_raw(
                vec![
                    FiberValue::I32(2),
                    FiberValue::I32(2),
                    FiberValue::I32(3),
                    FiberValue::I32(4),
                ]
                .into_boxed_slice(),
            )),
        );

        if let FiberValue::List(result) = state_get(mapped) {
            let result = unsafe { &*result };
            assert_eq!(
                result,
                &[
                    FiberValue::I32(4),
                    FiberValue::I32(4),
                    FiberValue::I32(6),
                    FiberValue::I32(8),
                ]
            );
        } else {
            panic!("Expected list");
        }
    }

    #[test]
    fn test_map_indexed_clear() {
        let list = use_state(FiberValue::List(Box::into_raw(
            vec![FiberValue::I32(1), FiberValue::I32(2), FiberValue::I32(3)].into_boxed_slice(),
        )));

        let mapped = map_indexed(list, double_map);

        state_set(
            list,
            FiberValue::List(Box::into_raw(Vec::new().into_boxed_slice())),
        );

        if let FiberValue::List(result) = state_get(mapped) {
            let result = unsafe { &*result };
            assert!(result.is_empty());
        } else {
            panic!("Expected list");
        }
    }

    #[test]
    fn test_map_indexed_react() {
        let list = use_state(FiberValue::List(Box::into_raw(
            vec![FiberValue::I32(1), FiberValue::I32(2), FiberValue::I32(3)].into_boxed_slice(),
        )));

        let mapped = map_indexed(list, double_map);
        let counter = use_state(FiberValue::I32(0));

        create_effect(move || {
            state_set(
                counter,
                FiberValue::I32(match state_get(counter) {
                    FiberValue::I32(v) => v + 1,
                    _ => 0,
                }),
            );
            state_get(mapped);
        });

        assert_eq!(state_get(counter), FiberValue::I32(1));

        state_set(
            list,
            FiberValue::List(Box::into_raw(
                vec![
                    FiberValue::I32(1),
                    FiberValue::I32(2),
                    FiberValue::I32(3),
                    FiberValue::I32(4),
                ]
                .into_boxed_slice(),
            )),
        );

        assert_eq!(state_get(counter), FiberValue::I32(2));
    }

    #[test]
    fn test_map_indexed_use_previous_computation() {
        let counter = Rc::new(Cell::new(0));
        let counter_clone = Rc::clone(&counter);

        extern "C" fn counter_map(_: usize, x: *const FiberValue) -> FiberValue {
            if x.is_null() {
                return FiberValue::Void;
            }
            let counter = unsafe { &*COUNTER };
            counter.set(counter.get() + 1);
            FiberValue::I32(counter.get())
        }

        static mut COUNTER: *const Cell<i32> = ptr::null();

        unsafe {
            COUNTER = Rc::into_raw(counter_clone) as *const Cell<i32>;
        }

        let list = use_state(FiberValue::List(Box::into_raw(
            vec![FiberValue::I32(1), FiberValue::I32(2), FiberValue::I32(3)].into_boxed_slice(),
        )));

        let mapped = map_indexed(list, counter_map);

        if let FiberValue::List(result) = state_get(mapped) {
            let result = unsafe { &*result };
            assert_eq!(
                result,
                &[FiberValue::I32(1), FiberValue::I32(2), FiberValue::I32(3)]
            );
        } else {
            panic!("Expected list");
        }

        state_set(
            list,
            FiberValue::List(Box::into_raw(
                vec![FiberValue::I32(1), FiberValue::I32(2)].into_boxed_slice(),
            )),
        );

        if let FiberValue::List(result) = state_get(mapped) {
            let result = unsafe { &*result };
            assert_eq!(result, &[FiberValue::I32(1), FiberValue::I32(2),]);
        } else {
            panic!("Expected list");
        }

        state_set(
            list,
            FiberValue::List(Box::into_raw(
                vec![FiberValue::I32(1), FiberValue::I32(2), FiberValue::I32(4)].into_boxed_slice(),
            )),
        );

        if let FiberValue::List(result) = state_get(mapped) {
            let result = unsafe { &*result };
            assert_eq!(
                result,
                &[FiberValue::I32(1), FiberValue::I32(2), FiberValue::I32(4)]
            );
        } else {
            panic!("Expected list");
        }

        state_set(
            list,
            FiberValue::List(Box::into_raw(
                vec![FiberValue::I32(1), FiberValue::I32(3), FiberValue::I32(4)].into_boxed_slice(),
            )),
        );

        if let FiberValue::List(result) = state_get(mapped) {
            let result = unsafe { &*result };
            assert_eq!(
                result,
                &[FiberValue::I32(1), FiberValue::I32(5), FiberValue::I32(4)]
            );
        } else {
            panic!("Expected list");
        }

        // Clean up the static counter
        unsafe {
            let _ = Rc::from_raw(COUNTER as *const Cell<i32>);
        }
    }
}
