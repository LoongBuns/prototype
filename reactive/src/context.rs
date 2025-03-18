use core::any::{Any, type_name};

use alloc::boxed::Box;
use slotmap::Key;

use super::*;

#[cfg_attr(debug_assertions, track_caller)]
pub fn provide_context<T: 'static>(value: T) {
    let root = Root::global();
    provide_context_in_node(root.current_node.get(), value);
}

pub fn provide_context_in_new_scope<T: 'static, U>(value: T, f: impl FnOnce() -> U) -> U {
    let mut result = None;
    create_child_scope(|| {
        provide_context(value);
        result = Some(f());
    });
    result.unwrap()
}

#[cfg_attr(debug_assertions, track_caller)]
fn provide_context_in_node<T: 'static>(id: NodeId, value: T) {
    let root = Root::global();
    let mut nodes = root.nodes.borrow_mut();
    let any: Box<dyn Any> = Box::new(value);

    let node = &mut nodes[id];
    if node
        .context
        .iter()
        .any(|x| (**x).type_id() == (*any).type_id())
    {
        panic!(
            "a context with type `{}` exists already in this scope",
            type_name::<T>()
        );
    }
    node.context.push(any);
}

#[cfg_attr(debug_assertions, track_caller)]
pub fn try_use_context<T: Clone + 'static>() -> Option<T> {
    let root = Root::global();
    let nodes = root.nodes.borrow();

    let mut current = Some(&nodes[root.current_node.get()]);
    while let Some(next) = current {
        for value in &next.context {
            if let Some(value) = value.downcast_ref::<T>().cloned() {
                return Some(value);
            }
        }

        if next.parent.is_null() {
            current = None;
        } else {
            current = Some(&nodes[next.parent]);
        }
    }
    None
}

#[cfg_attr(debug_assertions, track_caller)]
pub fn use_context<T: Clone + 'static>() -> T {
    if let Some(value) = try_use_context() {
        value
    } else {
        panic!("no context of type `{}` found", type_name::<T>())
    }
}

pub fn use_context_or_else<T: Clone + 'static, F: FnOnce() -> T>(f: F) -> T {
    try_use_context().unwrap_or_else(|| {
        let value = f();
        provide_context(value.clone());
        value
    })
}

pub fn use_scope_depth() -> u32 {
    let root = Root::global();
    let nodes = root.nodes.borrow();

    let mut current = Some(&nodes[root.current_node.get()]);
    let mut depth = 0;
    while let Some(next) = current {
        depth += 1;
        if next.parent.is_null() {
            current = None;
        } else {
            current = Some(&nodes[next.parent]);
        }
    }
    depth
}
