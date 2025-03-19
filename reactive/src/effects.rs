use core::cell::RefCell;

use alloc::boxed::Box;
use alloc::rc::Rc;

use super::*;

#[cfg_attr(debug_assertions, track_caller)]
pub fn use_effect(f: impl FnMut() + 'static) {
    use_memo(f);
}

#[cfg_attr(debug_assertions, track_caller)]
pub fn use_effect_deps<T: 'static>(deps: impl Trackable + 'static, f: impl FnMut() -> T + 'static) {
    use_memo(on(deps, f));
}

#[cfg_attr(debug_assertions, track_caller)]
pub fn use_effect_initial<T: 'static>(
    initial: impl FnOnce() -> (Box<dyn FnMut() + 'static>, T) + 'static,
) -> T {
    let result = Rc::new(RefCell::new(None));
    let mut initial = Some(initial);
    let mut effect = None;

    use_effect({
        let ret = Rc::clone(&result);
        move || {
            if let Some(initial) = initial.take() {
                let (new_f, value) = initial();
                effect = Some(new_f);
                *ret.borrow_mut() = Some(value);
            } else {
                effect.as_mut().unwrap()()
            }
        }
    });

    result.take().unwrap()
}
