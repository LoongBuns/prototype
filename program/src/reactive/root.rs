use core::cell::{Cell, RefCell};

use alloc::boxed::Box;
use alloc::vec::Vec;
use slotmap::{Key, SlotMap};
use smallvec::SmallVec;
use spin::Mutex;

use super::*;

pub(crate) struct Root {
    pub tracker: RefCell<Option<DependencyTracker>>,
    pub rev_sorted_buf: RefCell<Vec<NodeId>>,
    pub current_node: Cell<NodeId>,
    pub root_node: Cell<NodeId>,
    pub nodes: RefCell<SlotMap<NodeId, ReactiveNode>>,
    pub node_update_queue: RefCell<Vec<NodeId>>,
    pub batching: Cell<bool>,
}

unsafe impl Sync for Root {}

static GLOBAL_ROOT: Mutex<Option<&'static Root>> = Mutex::new(None);

impl Root {
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn global() -> &'static Root {
        GLOBAL_ROOT.lock().as_ref().expect("no root found")
    }

    pub fn set_global(root: Option<&'static Root>) -> Option<&'static Root> {
        let mut lock = GLOBAL_ROOT.lock();
        let prev = *lock;
        *lock = root;
        prev
    }

    pub fn new_static() -> &'static Self {
        let this = Self {
            tracker: RefCell::new(None),
            rev_sorted_buf: RefCell::new(Vec::new()),
            current_node: Cell::new(NodeId::null()),
            root_node: Cell::new(NodeId::null()),
            nodes: RefCell::new(SlotMap::default()),
            node_update_queue: RefCell::new(Vec::new()),
            batching: Cell::new(false),
        };
        let _ref = Box::leak(Box::new(this));
        _ref.reinit();
        _ref
    }

    pub fn reinit(&'static self) {
        NodeHandle(self.root_node.get(), self).dispose();

        let _ = self.tracker.take();
        let _ = self.rev_sorted_buf.take();
        let _ = self.node_update_queue.take();
        let _ = self.current_node.take();
        let _ = self.root_node.take();
        let _ = self.nodes.take();
        self.batching.set(false);

        Root::set_global(Some(self));
        let root_node = create_child_scope(|| {});
        Root::set_global(None);
        self.root_node.set(root_node.0);
        self.current_node.set(root_node.0);
    }

    pub fn create_child_scope(&'static self, f: impl FnOnce()) -> NodeHandle {
        let node = use_signal(()).id;
        let prev = self.current_node.replace(node);
        f();
        self.current_node.set(prev);
        NodeHandle(node, self)
    }

    pub fn tracked_scope<T>(&self, f: impl FnOnce() -> T) -> (T, DependencyTracker) {
        let prev = self.tracker.replace(Some(DependencyTracker::default()));
        let ret = f();
        (ret, self.tracker.replace(prev).unwrap())
    }

    fn run_node_update(&'static self, current: NodeId) {
        debug_assert_eq!(
            self.nodes.borrow()[current].state,
            NodeState::Dirty,
            "should only update when dirty"
        );

        let dependencies = core::mem::take(&mut self.nodes.borrow_mut()[current].dependencies);
        for dependency in dependencies {
            self.nodes.borrow_mut()[dependency]
                .dependents
                .retain(|&id| id != current);
        }

        let mut nodes_mut = self.nodes.borrow_mut();
        let mut callback = nodes_mut[current].callback.take().unwrap();
        let mut value = nodes_mut[current].value.take().unwrap();
        drop(nodes_mut);

        NodeHandle(current, self).dispose_children();

        let prev = self.current_node.replace(current);
        let (changed, tracker) = self.tracked_scope(|| callback(&mut value));
        self.current_node.set(prev);

        tracker.create_dependency_link(self, current);

        let mut nodes_mut = self.nodes.borrow_mut();
        nodes_mut[current].callback = Some(callback);
        nodes_mut[current].value = Some(value);

        nodes_mut[current].state = NodeState::Clean;
        drop(nodes_mut);

        if changed {
            self.mark_dependents_dirty(current);
        }
    }

    fn mark_dependents_dirty(&self, current: NodeId) {
        let mut nodes_mut = self.nodes.borrow_mut();
        let dependents = core::mem::take(&mut nodes_mut[current].dependents);
        for &dependent in &dependents {
            if let Some(dependent) = nodes_mut.get_mut(dependent) {
                dependent.state = NodeState::Dirty;
            }
        }
        nodes_mut[current].dependents = dependents;
    }

    fn propagate_node_updates(&'static self, start_nodes: &[NodeId]) {
        let mut rev_sorted = Vec::new();
        let mut rev_sorted_buf = self.rev_sorted_buf.try_borrow_mut();
        let rev_sorted = if let Ok(rev_sorted_buf) = rev_sorted_buf.as_mut() {
            rev_sorted_buf.clear();
            rev_sorted_buf
        } else {
            &mut rev_sorted
        };

        for &node in start_nodes {
            Self::dfs(node, &mut self.nodes.borrow_mut(), rev_sorted);
            self.mark_dependents_dirty(node);
        }

        for &node in rev_sorted.iter().rev() {
            let mut nodes_mut = self.nodes.borrow_mut();

            if nodes_mut.get(node).is_none() {
                continue;
            }
            let node_state = &mut nodes_mut[node];
            node_state.mark = Mark::None;

            if nodes_mut[node].state == NodeState::Dirty {
                drop(nodes_mut);
                self.run_node_update(node)
            };
        }
    }

    pub fn propagate_updates(&'static self, start_node: NodeId) {
        if self.batching.get() {
            self.node_update_queue.borrow_mut().push(start_node);
        } else {
            let prev = Root::set_global(Some(self));

            self.propagate_node_updates(&[start_node]);
            Root::set_global(prev);
        }
    }

    fn dfs(current_id: NodeId, nodes: &mut SlotMap<NodeId, ReactiveNode>, buf: &mut Vec<NodeId>) {
        let Some(current) = nodes.get_mut(current_id) else {
            return;
        };

        match current.mark {
            Mark::Temp => panic!("cyclic reactive dependency"),
            Mark::Permanent => return,
            Mark::None => {}
        }
        current.mark = Mark::Temp;

        let children = core::mem::take(&mut current.dependents);
        for child in &children {
            Self::dfs(*child, nodes, buf);
        }
        nodes[current_id].dependents = children;

        nodes[current_id].mark = Mark::Permanent;
        buf.push(current_id);
    }

    fn start_batch(&self) {
        self.batching.set(true);
    }

    fn end_batch(&'static self) {
        self.batching.set(false);
        let nodes = self.node_update_queue.take();
        self.propagate_node_updates(&nodes);
    }
}

#[derive(Clone, Copy)]
pub struct RootHandle {
    _ref: &'static Root,
}

impl RootHandle {
    pub fn dispose(&self) {
        self._ref.reinit();
    }

    pub fn run_in<T>(&self, f: impl FnOnce() -> T) -> T {
        let prev = Root::set_global(Some(self._ref));
        let ret = f();
        Root::set_global(prev);
        ret
    }
}

#[derive(Default)]
pub(crate) struct DependencyTracker {
    pub dependencies: SmallVec<[NodeId; 1]>,
}

impl DependencyTracker {
    pub fn create_dependency_link(self, root: &Root, dependent: NodeId) {
        for node in &self.dependencies {
            root.nodes.borrow_mut()[*node].dependents.push(dependent);
        }

        root.nodes.borrow_mut()[dependent].dependencies = self.dependencies;
    }
}

#[must_use = "root should be disposed"]
pub fn create_root(f: impl FnOnce()) -> RootHandle {
    let _ref = Root::new_static();

    Root::set_global(Some(_ref));
    NodeHandle(_ref.root_node.get(), _ref).run_in(f);
    Root::set_global(None);
    RootHandle { _ref }
}

#[cfg_attr(debug_assertions, track_caller)]
pub fn create_child_scope(f: impl FnOnce()) -> NodeHandle {
    Root::global().create_child_scope(f)
}

#[cfg_attr(debug_assertions, track_caller)]
pub fn on_cleanup(f: impl FnOnce() + 'static) {
    let root = Root::global();
    if !root.current_node.get().is_null() {
        root.nodes.borrow_mut()[root.current_node.get()]
            .cleanups
            .push(Box::new(f));
    }
}

pub fn batch<T>(f: impl FnOnce() -> T) -> T {
    let root = Root::global();
    root.start_batch();
    let ret = f();
    root.end_batch();
    ret
}

pub fn untrack<T>(f: impl FnOnce() -> T) -> T {
    untrack_in_scope(f, Root::global())
}

pub(crate) fn untrack_in_scope<T>(f: impl FnOnce() -> T, root: &'static Root) -> T {
    let prev = root.tracker.replace(None);
    let ret = f();
    root.tracker.replace(prev);
    ret
}

pub fn use_current_scope() -> NodeHandle {
    let root = Root::global();
    NodeHandle(root.current_node.get(), root)
}

pub fn use_global_scope() -> NodeHandle {
    let root = Root::global();
    NodeHandle(root.root_node.get(), root)
}
