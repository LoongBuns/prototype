use core::cell::RefCell;

use alloc::boxed::Box;

use super::*;

#[cfg_attr(debug_assertions, track_caller)]
pub fn use_selector_with<T>(
    mut f: impl FnMut() -> T + 'static,
    mut eq: impl FnMut(&T, &T) -> bool + 'static,
) -> ReadSignal<T> {
    let root = Root::global();
    let signal = create_empty_signal();
    let prev = root.current_node.replace(signal.id);
    let (initial, tracker) = root.tracked_scope(&mut f);
    root.current_node.set(prev);

    tracker.create_dependency_link(root, signal.id);

    let mut signal_mut = signal.get_mut();
    signal_mut.value = Some(Box::new(initial));
    signal_mut.callback = Some(Box::new(move |value| {
        let value = value.downcast_mut().expect("Type mismatch in memo");
        let new = f();
        if eq(&new, value) {
            false
        } else {
            *value = new;
            true
        }
    }));

    *signal
}

#[cfg_attr(debug_assertions, track_caller)]
pub fn use_memo<T>(f: impl FnMut() -> T + 'static) -> ReadSignal<T> {
    use_selector_with(f, |_, _| false)
}

#[cfg_attr(debug_assertions, track_caller)]
pub fn use_selector<T>(f: impl FnMut() -> T + 'static) -> ReadSignal<T>
where
    T: PartialEq,
{
    use_selector_with(f, PartialEq::eq)
}

#[cfg_attr(debug_assertions, track_caller)]
pub fn use_reducer<T, Msg>(
    initial: T,
    reduce: impl FnMut(&T, Msg) -> T,
) -> (ReadSignal<T>, impl Fn(Msg)) {
    let reduce = RefCell::new(reduce);
    let signal = use_signal(initial);
    let dispatch = move |msg| signal.update(|value| *value = reduce.borrow_mut()(value, msg));
    (*signal, dispatch)
}
