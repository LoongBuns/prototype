#[macro_use]
extern crate alloc;

mod effect;
mod iter;
mod state;

use core::ffi;
pub use effect::*;
pub use iter::*;
pub use state::*;

#[derive(Debug, Default, Clone, PartialOrd, PartialEq)]
#[repr(C)]
pub enum CValue {
    #[default]
    Void,
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    List(*mut [CValue]),
}

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

#[repr(C)]
pub struct CStateHandle(StateHandle<CValue>);

#[unsafe(no_mangle)]
pub extern "C" fn use_state(value: CValue) -> *mut CStateHandle {
    Box::into_raw(Box::new(CStateHandle(StateHandle::new(value))))
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
pub extern "C" fn use_effect(cx: *mut ffi::c_void, effect: extern "C" fn(*mut ffi::c_void)) {
    fn internal(mut effect: Box<dyn FnMut()>) {
        create_effect_dyn(Box::new(|| {
            effect();
            (Box::new(effect), Box::new(()))
        }));
    }

    internal(Box::new(move || effect(cx)));
}

#[cfg(test)]
mod tests {
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

        extern "C" fn effect_callback(context: *mut ffi::c_void) {
            let context = unsafe { &*(context as *const EffectContext) };
            let value = state_get(context.state);
            if let CValue::I32(v) = value {
                state_set(context.double, CValue::I32(v * 2));
            }
        }

        let context = Box::new(EffectContext { state, double });
        let context_ptr = Box::into_raw(context);
        use_effect(context_ptr as *mut ffi::c_void, effect_callback);

        assert_eq!(state_get(state), CValue::I32(0));
        assert_eq!(state_get(double), CValue::I32(0));

        state_set(state, CValue::I32(1));
        assert_eq!(state_get(double), CValue::I32(2));

        state_set(state, CValue::I32(2));
        assert_eq!(state_get(double), CValue::I32(4));
    }
}
