use core::any::Any;
use core::cell::RefCell;
use core::hash::{Hash, Hasher};
use core::{mem, ptr};

use alloc::rc::{Rc, Weak};

use hashbrown::HashSet;

use super::create_root;
use super::state::SignalEmitter;

thread_local! {
    pub(super) static CONTEXTS: RefCell<Vec<Weak<RefCell<Option<Effect>>>>> = const { RefCell::new(Vec::new()) };
    pub(super) static OWNER: RefCell<Option<Scope>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub(super) struct Dependency(pub(super) Rc<dyn SignalEmitter>);

impl Dependency {
    fn signal(&self) -> Rc<dyn SignalEmitter> {
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

pub(super) struct Effect {
    pub(super) execute: Rc<RefCell<dyn FnMut()>>,
    pub(super) dependencies: HashSet<Dependency>,
    scope: Scope,
}

impl Effect {
    pub fn add_dependency(&mut self, signal: Rc<dyn SignalEmitter>) {
        self.dependencies.insert(Dependency(signal));
    }

    fn clear_dependencies(&mut self) {
        for dependency in &self.dependencies {
            dependency.signal().unsubscribe(Rc::as_ptr(&self.execute));
        }
        self.dependencies.clear();
    }
}

#[derive(Default)]
pub struct Scope {
    effects: Vec<Rc<RefCell<Option<Effect>>>>,
    cleanup: Vec<Box<dyn FnOnce()>>,
}

impl Scope {
    pub(super) fn add_effect(&mut self, effect: Rc<RefCell<Option<Effect>>>) {
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

pub(super) fn create_effect_dyn(
    initial: Box<dyn FnOnce() -> (Box<dyn FnMut()>, Box<dyn Any>)>,
) -> Box<dyn Any> {
    let running: Rc<RefCell<Option<Effect>>> = Rc::new(RefCell::new(None));

    let mut effect: Option<Box<dyn FnMut()>> = None;
    let ret: Rc<RefCell<Option<Box<dyn Any>>>> = Rc::new(RefCell::new(None));

    let mut initial = Some(initial);

    let execute: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new({
        let running = Rc::downgrade(&running);
        let ret = Rc::downgrade(&ret);
        move || {
            CONTEXTS.with(|effects| {
                let initial_context_size = effects.borrow().len();

                // Upgrade running now to make sure running is valid for the whole duration of the effect.
                let running = running.upgrade().unwrap();

                // Push new reactive scope.
                effects.borrow_mut().push(Rc::downgrade(&running));

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
                        .subscribe(Rc::downgrade(&running.execute));
                }

                // Remove reactive context.
                effects.borrow_mut().pop();

                debug_assert_eq!(
                    initial_context_size,
                    effects.borrow().len(),
                    "context size should not change before and after create_effect_initial"
                );
            });
        }
    }));

    *running.borrow_mut() = Some(Effect {
        execute: Rc::clone(&execute),
        dependencies: HashSet::new(),
        scope: Default::default(),
    });
    debug_assert_eq!(
        Rc::strong_count(&running),
        1,
        "Running should be owned exclusively by Scope"
    );

    OWNER.with(|scope| {
        if scope.borrow().is_some() {
            scope.borrow_mut().as_mut().unwrap().add_effect(running);
        } else {
            let _ = Rc::into_raw(running); // leak running
        }
    });

    execute.borrow_mut()();

    let ret = Rc::try_unwrap(ret).unwrap();
    ret.into_inner().unwrap()
}

pub fn create_effect_init<R: 'static>(
    initial: impl FnOnce() -> (Box<dyn FnMut()>, R) + 'static,
) -> R {
    let ret = create_effect_dyn(Box::new(|| {
        let (effect, ret) = initial();
        (effect, Box::new(ret))
    }));

    *ret.downcast::<R>().unwrap()
}

pub fn create_effect<F>(mut effect: F)
where
    F: FnMut() + 'static,
{
    create_effect_dyn(Box::new(|| {
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
        let trigger = StateHandle::new(());
        let state = StateHandle::new(0);
        let double = StateHandle::new(-1);

        create_effect({
            let trigger = trigger.clone();
            let state = state.clone();
            let double = double.clone();
            move || {
                trigger.track();
                double.set(*state.get_tracked() * 2);
            }
        });

        assert_eq!(*double.get_tracked(), 0);

        state.set(1);
        assert_eq!(*double.get_tracked(), 2);
        state.set(2);
        assert_eq!(*double.get_tracked(), 4);

        trigger.set(());
        assert_eq!(*double.get_tracked(), 4);
    }

    #[test]
    fn test_effect_no_infinite_loop() {
        let state = StateHandle::new(0);

        create_effect({
            let state = state.clone();
            move || {
                state.track();
                state.set(0);
            }
        });

        state.set(0);
    }

    #[test]
    fn test_effect_should_only_subscribe_once() {
        let state = StateHandle::new(0);
        let counter = StateHandle::new(0);

        create_effect({
            let counter = counter.clone();
            let state = state.clone();
            move || {
                let result = *counter.get() + 1;
                counter.set(result);

                state.get_tracked();
                state.get_tracked();
            }
        });

        assert_eq!(*counter.get_tracked(), 1);

        state.set(1);
        assert_eq!(*counter.get_tracked(), 2);
    }

    #[test]
    fn test_effect_should_recreate_dependencies() {
        let condition = StateHandle::new(true);
        let state1 = StateHandle::new(0);
        let state2 = StateHandle::new(1);
        let counter = StateHandle::new(0);

        create_effect({
            let condition = condition.clone();
            let state1 = state1.clone();
            let state2 = state2.clone();
            let counter = counter.clone();
            move || {
                counter.set(*counter.get() + 1);

                if *condition.get_tracked() {
                    state1.track();
                } else {
                    state2.track();
                }
            }
        });

        assert_eq!(*counter.get_tracked(), 1);

        state1.set(1);
        assert_eq!(*counter.get_tracked(), 2);

        state2.set(1);
        assert_eq!(*counter.get_tracked(), 2);

        condition.set(false);
        assert_eq!(*counter.get_tracked(), 3);

        state1.set(2);
        assert_eq!(*counter.get_tracked(), 3);

        state2.set(2);
        assert_eq!(*counter.get_tracked(), 4);
    }

    #[test]
    fn test_effect_should_recreate_nested_inner() {
        let outer_counter = StateHandle::new(0);
        let inner_counter = StateHandle::new(0);
        let trigger = StateHandle::new(());

        create_effect({
            let outer_counter = outer_counter.clone();
            let inner_counter = inner_counter.clone();
            let trigger = trigger.clone();
            move || {
                trigger.track();
                outer_counter.set(*outer_counter.get() + 1);

                create_effect({
                    let inner_counter = inner_counter.clone();
                    let trigger = trigger.clone();
                    move || {
                        trigger.track();
                        inner_counter.set(*inner_counter.get() + 1);
                    }
                });
            }
        });

        assert_eq!(*outer_counter.get_tracked(), 1);
        assert_eq!(*inner_counter.get_tracked(), 1);

        trigger.set(());

        assert_eq!(*outer_counter.get_tracked(), 2);
        assert_eq!(*inner_counter.get_tracked(), 2);
    }

    #[test]
    fn test_state_cleanup() {
        let counter = StateHandle::new(0);
        let state = StateHandle::new(0);

        create_effect({
            let counter = counter.clone();
            let state = state.clone();
            move || {
                state.track();
                on_cleanup({
                    let counter = counter.clone();
                    move || {
                        counter.set(*counter.get() + 1);
                    }
                });
            }
        });

        assert_eq!(*counter.get_tracked(), 0);

        state.set(1);
        assert_eq!(*counter.get_tracked(), 1);

        state.set(2);
        assert_eq!(*counter.get_tracked(), 2);
    }
}
