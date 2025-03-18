use core::any::Any;
use core::cell::RefCell;
use core::hash::{Hash, Hasher};
use core::{mem, ptr};

use alloc::rc::{Rc, Weak};

use hashbrown::HashSet;

use super::create_root;
use super::state::DynSignalInner;

thread_local! {
    pub(super) static CONTEXTS: RefCell<Vec<Weak<RefCell<Option<Fiber>>>>> = RefCell::new(Vec::new());
    pub(super) static OWNER: RefCell<Option<Scope>> = RefCell::new(None);
}

pub(super) struct Fiber {
    pub(super) execute: Rc<RefCell<dyn FnMut()>>,
    pub(super) dependencies: HashSet<Dependency>,
    scope: Scope,
}

impl Fiber {
    fn clear_dependencies(&mut self) {
        for dependency in &self.dependencies {
            dependency.signal().unsubscribe(Rc::as_ptr(&self.execute));
        }
        self.dependencies.clear();
    }
}

#[derive(Default)]
pub struct Scope {
    effects: Vec<Rc<RefCell<Option<Fiber>>>>,
    cleanup: Vec<Box<dyn FnOnce()>>,
}

impl Scope {
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) fn add_effect_state(&mut self, effect: Rc<RefCell<Option<Fiber>>>) {
        self.effects.push(effect);
    }

    pub(super) fn add_cleanup(&mut self, cleanup: Box<dyn FnOnce()>) {
        self.cleanup.push(cleanup);
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        for effect in &self.effects {
            effect.borrow_mut().as_mut().unwrap().clear_dependencies();
        }

        for cleanup in mem::take(&mut self.cleanup) {
            untrack(cleanup);
        }
    }
}

pub(super) type CallbackPtr = *const RefCell<dyn FnMut()>;

#[derive(Clone)]
pub(super) struct Callback(pub(super) Weak<RefCell<dyn FnMut()>>);

impl Callback {
    #[must_use = "returned value must be manually called"]
    pub fn callback(&self) -> Option<Rc<RefCell<dyn FnMut()>>> {
        self.0.upgrade()
    }

    pub fn as_ptr(&self) -> CallbackPtr {
        Weak::as_ptr(&self.0)
    }
}

#[derive(Clone)]
pub(super) struct Dependency(pub(super) Rc<dyn DynSignalInner>);

impl Dependency {
    fn signal(&self) -> Rc<dyn DynSignalInner> {
        Rc::clone(&self.0)
    }
}

impl Hash for Dependency {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

impl PartialEq for Dependency {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq::<()>(Rc::as_ptr(&self.0).cast(), Rc::as_ptr(&other.0).cast())
    }
}

impl Eq for Dependency {}

fn create_effect_internal(
    initial: Box<dyn FnOnce() -> (Box<dyn FnMut()>, Box<dyn Any>)>,
) -> Box<dyn Any> {
    let running: Rc<RefCell<Option<Fiber>>> = Rc::new(RefCell::new(None));

    let mut effect: Option<Box<dyn FnMut()>> = None;
    let ret: Rc<RefCell<Option<Box<dyn Any>>>> = Rc::new(RefCell::new(None));

    let mut initial = Some(initial);

    let execute: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new({
        let running = Rc::downgrade(&running);
        let ret = Rc::downgrade(&ret);
        move || {
            CONTEXTS.with(|contexts| {
                let initial_context_size = contexts.borrow().len();

                // Upgrade running now to make sure running is valid for the whole duration of the effect.
                let running = running.upgrade().unwrap();

                // Push new reactive scope.
                contexts.borrow_mut().push(Rc::downgrade(&running));

                if let Some(initial) = initial.take() {
                    // Call initial callback.
                    let ret = Weak::upgrade(&ret).unwrap();
                    let scope = create_root(|| {
                        let (effect_tmp, ret_tmp) = initial(); // Call initial callback.
                        effect = Some(effect_tmp);
                        *ret.borrow_mut() = Some(ret_tmp);
                    });
                    running.borrow_mut().as_mut().unwrap().scope = scope;
                } else {
                    // Recreate effect dependencies each time effect is called.
                    running.borrow_mut().as_mut().unwrap().clear_dependencies();

                    // Destroy old effects before new ones run.
                    mem::take(&mut running.borrow_mut().as_mut().unwrap().scope);

                    // Run effect closure.
                    let scope = create_root(|| {
                        effect.as_mut().unwrap()();
                    });
                    running.borrow_mut().as_mut().unwrap().scope = scope;
                }

                // Attach new dependencies.
                let running = running.borrow();
                let running = running.as_ref().unwrap();
                for dependency in &running.dependencies {
                    dependency
                        .signal()
                        .subscribe(Callback(Rc::downgrade(&running.execute)));
                }

                // Remove reactive context.
                contexts.borrow_mut().pop();

                debug_assert_eq!(
                    initial_context_size,
                    contexts.borrow().len(),
                    "context size should not change before and after create_effect_initial"
                );
            });
        }
    }));

    *running.borrow_mut() = Some(Fiber {
        execute: Rc::clone(&execute),
        dependencies: HashSet::new(),
        scope: Scope::new(),
    });
    debug_assert_eq!(
        Rc::strong_count(&running),
        1,
        "Running should be owned exclusively by Scope"
    );

    OWNER.with(|scope| {
        if scope.borrow().is_some() {
            scope
                .borrow_mut()
                .as_mut()
                .unwrap()
                .add_effect_state(running);
        } else {
            Rc::into_raw(running); // leak running
        }
    });

    execute.borrow_mut()();

    let ret = Rc::try_unwrap(ret).unwrap();
    ret.into_inner().unwrap()
}

#[unsafe(no_mangle)]
pub extern "C" fn use_effect(effect: extern "C" fn()) {
    fn internal(mut effect: Box<dyn FnMut()>) {
        create_effect_internal(Box::new(|| {
            effect();
            (Box::new(effect), Box::new(()))
        }));
    }

    internal(Box::new(move || effect()));
}

pub fn create_effect_init<R: 'static>(
    initial: impl FnOnce() -> (Box<dyn FnMut()>, R) + 'static,
) -> R {
    let ret = create_effect_internal(Box::new(|| {
        let (effect, ret) = initial();
        (effect, Box::new(ret))
    }));

    *ret.downcast::<R>().unwrap()
}

pub fn create_effect<F>(mut effect: F)
where
    F: FnMut() + 'static,
{
    create_effect_internal(Box::new(|| {
        effect();
        (Box::new(effect), Box::new(()))
    }));
}

pub fn untrack<T>(f: impl FnOnce() -> T) -> T {
    let f = Rc::new(RefCell::new(Some(f)));
    let g = Rc::clone(&f);

    if let Ok(ret) = CONTEXTS.try_with(|contexts| {
        let tmp = contexts.take();

        let ret = f.take().unwrap()();

        *contexts.borrow_mut() = tmp;

        ret
    }) {
        ret
    } else {
        g.take().unwrap()()
    }
}

pub fn on_cleanup(f: impl FnOnce() + 'static) {
    OWNER.with(|scope| {
        if scope.borrow().is_some() {
            scope
                .borrow_mut()
                .as_mut()
                .unwrap()
                .add_cleanup(Box::new(f));
        }
    });
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_effect() {
        let state = use_state(FiberValue::I32(0));
        let double = use_state(FiberValue::I32(-1));

        create_effect(move || {
            let result = match state_get(state) {
                FiberValue::I32(v) => v * 2,
                _ => 0,
            };
            state_set(state, FiberValue::I32(result));
        });
        assert_eq!(state_get(state), FiberValue::I32(0));

        state_set(state, FiberValue::I32(1));
        assert_eq!(state_get(state), FiberValue::I32(2));

        state_set(state, FiberValue::I32(2));
        assert_eq!(state_get(state), FiberValue::I32(4));
    }

    #[test]
    fn test_effect_should_only_subscribe_once() {
        let state = use_state(FiberValue::I32(0));
        let counter = use_state(FiberValue::I32(0));

        create_effect(move || {
            let result = match state_get_raw(counter) {
                FiberValue::I32(v) => v + 1,
                _ => 0,
            };
            state_set(counter, FiberValue::I32(result));

            state_get(state);
            state_get(state);
        });

        assert_eq!(state_get(counter), FiberValue::I32(1));

        state_set(state, FiberValue::I32(1));
        assert_eq!(state_get(counter), FiberValue::I32(2));
    }

    #[test]
    fn test_effect_should_recreate_dependencies() {
        let condition = use_state(FiberValue::I32(0));
        let state1 = use_state(FiberValue::I32(0));
        let state2 = use_state(FiberValue::I32(1));
        let counter = use_state(FiberValue::I32(0));

        create_effect(move || {
            let result = match state_get_raw(counter) {
                FiberValue::I32(v) => v + 1,
                _ => 0,
            };
            state_set(counter, FiberValue::I32(result));

            if matches!(state_get(condition), FiberValue::I32(0)) {
                state_get(state1);
            } else {
                state_get(state2);
            }
        });

        assert_eq!(state_get(counter), FiberValue::I32(1));

        state_set(state1, FiberValue::I32(1));
        assert_eq!(state_get(counter), FiberValue::I32(2));

        state_set(state2, FiberValue::I32(1));
        assert_eq!(state_get(counter), FiberValue::I32(2));

        state_set(condition, FiberValue::I32(1));
        assert_eq!(state_get(counter), FiberValue::I32(3));

        state_set(state1, FiberValue::I32(2));
        assert_eq!(state_get(counter), FiberValue::I32(3));

        state_set(state2, FiberValue::I32(2));
        assert_eq!(state_get(counter), FiberValue::I32(4));
    }

    #[test]
    fn test_effect_should_recreate_nested_inner() {
        let counter = use_state(FiberValue::I32(0));
        let trigger = use_state(FiberValue::Void);

        create_effect(move || {
            state_get(trigger); // subscribe to trigger

            create_effect(move || {
                let result = match state_get_raw(counter) {
                    FiberValue::I32(v) => v + 1,
                    _ => 0,
                };
                state_set(counter, FiberValue::I32(result));
            });
        });

        assert_eq!(state_get(counter), FiberValue::I32(1));

        state_set(trigger, FiberValue::Void);
        assert_eq!(state_get(counter), FiberValue::I32(2));
    }
}
