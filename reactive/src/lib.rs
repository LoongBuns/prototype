#[macro_use]
extern crate alloc;

mod effect;
mod iter;
mod state;

use core::{ffi, mem, ptr};

pub use effect::*;
pub use iter::*;
pub use state::*;

#[must_use = "create_root returns the owner of the effects created inside this scope"]
pub fn create_root<'a>(callback: impl FnOnce() + 'a) -> Scope {
    fn internal<'a>(callback: Box<dyn FnOnce() + 'a>) -> Scope {
        OWNER.with(|scope| {
            let outer_scope = scope.replace(Some(Default::default()));
            callback();

            scope
                .replace(outer_scope)
                .expect("Owner should be valid inside the reactive root")
        })
    }

    internal(Box::new(callback))
}

#[derive(Debug, Default, Clone, PartialOrd, PartialEq)]
#[repr(C)]
pub enum CValue {
    #[default]
    Void,
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

#[derive(Debug, Clone, PartialOrd, PartialEq)]
#[repr(C)]
pub struct CValueBuffer {
    data: *mut CValue,
    len: usize,
    capacity: usize,
}

#[repr(C)]
pub struct CStateHandle(StateHandle<CValue>);

#[repr(C)]
pub struct CStateBufHandle(StateHandle<Vec<CValue>>);

#[unsafe(no_mangle)]
pub extern "C" fn use_state(value: CValue) -> *mut CStateHandle {
    Box::into_raw(Box::new(CStateHandle(StateHandle::new(value))))
}

#[unsafe(no_mangle)]
pub extern "C" fn use_state_buf(buffer: CValueBuffer) -> *mut CStateBufHandle {
    let vec = unsafe { Vec::from_raw_parts(buffer.data, buffer.len, buffer.capacity) };
    Box::into_raw(Box::new(CStateBufHandle(StateHandle::new(vec))))
}

#[unsafe(no_mangle)]
pub extern "C" fn state_get(handle: *const CStateHandle) -> CValue {
    if !handle.is_null() {
        let signal = unsafe { &(*(handle)).0 };
        (*signal.get_tracked()).clone()
    } else {
        CValue::Void
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn state_buf_get(handle: *const CStateBufHandle) -> CValueBuffer {
    if !handle.is_null() {
        let signal = unsafe { &(*(handle)).0 };
        let mut vec = (*signal.get_tracked()).clone();
        vec.shrink_to_fit();

        let data = vec.as_mut_ptr();
        let len = vec.len();
        let capacity = vec.capacity();
        mem::forget(vec);

        CValueBuffer { data, len, capacity }
    } else {
        CValueBuffer {
            data: ptr::null_mut(),
            len: 0,
            capacity: 0,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn state_get_raw(handle: *const CStateHandle) -> CValue {
    if !handle.is_null() {
        let signal = unsafe { &(*(handle)).0 };
        (*signal.get()).clone()
    } else {
        CValue::Void
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn state_set(handle: *mut CStateHandle, value: CValue) {
    let signal = unsafe { &(*(handle)).0 };
    signal.set(value);
}

#[unsafe(no_mangle)]
pub extern "C" fn state_buf_set(handle: *mut CStateBufHandle, buffer: CValueBuffer) {
    let vec = unsafe { Vec::from_raw_parts(buffer.data, buffer.len, buffer.capacity) };
    let signal = unsafe { &(*(handle)).0 };
    signal.set(vec);
}

#[unsafe(no_mangle)]
pub extern "C" fn use_effect(cx: *mut ffi::c_void, effect: extern "C" fn(*mut ffi::c_void)) {
    fn internal(mut effect: Box<dyn FnMut()>) {
        create_effect_dyn(Box::new(|| {
            effect();
            (Box::new(effect), Box::new(()))
        }));
    }

    internal(Box::new(move || effect(cx)));
}

type MapFn = extern "C" fn(*const CValue) -> CValue;

#[unsafe(no_mangle)]
pub extern "C" fn use_list(handle: *mut CStateBufHandle, map_fn: MapFn) -> *mut CStateBufHandle {
    if !handle.is_null() {
        let mapped_state = StateHandle::new(Vec::new());

        create_effect({
            let input_state = unsafe { &(*(handle)).0 };
            let mapped_state = mapped_state.clone();
            let map_fn = move |item: &CValue| map_fn(item);
            move || {
                let mut mapped = map_indexed(input_state.clone(), map_fn);
                mapped_state.set(mapped());
            }
        });

        Box::into_raw(Box::new(CStateBufHandle(mapped_state)))
    } else {
        ptr::null_mut()
    }
}

#[cfg(test)]
mod tests {
    use core::slice;

    use super::*;

    #[test]
    fn test_effect_ffi() {
        let state = use_state(CValue::I32(0));
        let double = use_state(CValue::I32(-1));

        #[repr(C)]
        struct EffectContext {
            state: *mut CStateHandle,
            double: *mut CStateHandle,
        }

        extern "C" fn effect(context: *mut ffi::c_void) {
            let context = unsafe { &*(context as *const EffectContext) };
            let value = state_get(context.state);
            if let CValue::I32(v) = value {
                state_set(context.double, CValue::I32(v * 2));
            }
        }

        let context = Box::new(EffectContext { state, double });
        let context_ptr = Box::into_raw(context);
        use_effect(context_ptr as *mut ffi::c_void, effect);

        assert_eq!(state_get(state), CValue::I32(0));
        assert_eq!(state_get(double), CValue::I32(0));

        state_set(state, CValue::I32(1));
        assert_eq!(state_get(double), CValue::I32(2));

        state_set(state, CValue::I32(2));
        assert_eq!(state_get(double), CValue::I32(4));
    }

    #[test]
    fn test_map_indexed_ffi() {
        fn create_buffer(mut buffer: Vec<CValue>) -> CValueBuffer {
            let len = buffer.len();
            let capacity = buffer.capacity();
            let data = buffer.as_mut_ptr();
            mem::forget(buffer);
            CValueBuffer { data, len, capacity }
        }

        extern "C" fn double_map(x: *const CValue) -> CValue {
            if let Some(CValue::I32(v)) = unsafe { x.as_ref() } {
                CValue::I32(v * 2)
            } else {
                CValue::Void
            }
        }

        let initial = vec![CValue::I32(1), CValue::I32(2), CValue::I32(3)];
        let list = use_state_buf(create_buffer(initial));

        let mapped = use_list(list, double_map);

        let result_buf = state_buf_get(mapped);
        let result = unsafe { slice::from_raw_parts(result_buf.data, result_buf.len) };
        assert_eq!(result, &[CValue::I32(2), CValue::I32(4), CValue::I32(6)]);

        let append = vec![CValue::I32(1), CValue::I32(2), CValue::I32(3), CValue::I32(4)];
        state_buf_set(list, create_buffer(append));

        let result_buf = state_buf_get(mapped);
        let result = unsafe { slice::from_raw_parts(result_buf.data, result_buf.len) };
        assert_eq!(result, &[CValue::I32(2), CValue::I32(4), CValue::I32(6), CValue::I32(8)]);

        let update = vec![CValue::I32(2), CValue::I32(2), CValue::I32(3), CValue::I32(4)];
        state_buf_set(list, create_buffer(update));

        let result_buf = state_buf_get(mapped);
        let result = unsafe { slice::from_raw_parts(result_buf.data, result_buf.len) };
        assert_eq!(result, &[CValue::I32(4), CValue::I32(4), CValue::I32(6), CValue::I32(8)]);
    }
}
