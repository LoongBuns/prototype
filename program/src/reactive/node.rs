use core::any::Any;

use alloc::boxed::Box;
use alloc::vec::Vec;
use slotmap::new_key_type;
use smallvec::SmallVec;

use super::*;

new_key_type! {
    pub(crate) struct NodeId;
}

pub(crate) struct ReactiveNode {
    pub value: Option<Box<dyn Any>>,
    pub callback: Option<Box<dyn FnMut(&mut Box<dyn Any>) -> bool>>,
    pub children: Vec<NodeId>,
    pub parent: NodeId,
    pub dependents: Vec<NodeId>,
    pub dependencies: SmallVec<[NodeId; 1]>,
    pub cleanups: Vec<Box<dyn FnOnce()>>,
    pub context: Vec<Box<dyn Any>>,
    pub state: NodeState,
    pub mark: Mark,
    #[cfg(debug_assertions)]
    #[allow(dead_code)]
    pub created_at: &'static core::panic::Location<'static>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeState {
    Clean,
    Dirty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    Temp,
    Permanent,
    None,
}

#[derive(Clone, Copy)]
pub struct NodeHandle(pub(crate) NodeId, pub(crate) &'static Root);

impl NodeHandle {
    pub fn dispose(self) {
        self.dispose_children();
        let mut nodes = self.1.nodes.borrow_mut();
        if let Some(this) = nodes.remove(self.0) {
            for dependent in this.dependents {
                if let Some(dependent) = nodes.get_mut(dependent) {
                    dependent.dependencies.retain(|&mut id| id != self.0);
                }
            }
        }
    }

    pub fn dispose_children(self) {
        if self.1.nodes.borrow().get(self.0).is_none() {
            return;
        }
        let cleanup = core::mem::take(&mut self.1.nodes.borrow_mut()[self.0].cleanups);
        let children = core::mem::take(&mut self.1.nodes.borrow_mut()[self.0].children);

        untrack_in_scope(
            move || {
                for cb in cleanup {
                    cb();
                }
            },
            self.1,
        );
        for child in children {
            Self(child, self.1).dispose();
        }

        self.1.nodes.borrow_mut()[self.0].context.clear();
    }

    pub fn run_in<T>(&self, f: impl FnOnce() -> T) -> T {
        let root = self.1;
        let prev_root = Root::set_global(Some(root));
        let prev_node = root.current_node.replace(self.0);
        let ret = f();
        root.current_node.set(prev_node);
        Root::set_global(prev_root);
        ret
    }
}
