use core::cell::{Ref, RefMut};
use core::fmt::{self, Formatter};
use core::hash::Hash;
use core::marker::PhantomData;
use core::ops::{AddAssign, Deref, DivAssign, MulAssign, RemAssign, SubAssign};

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use slotmap::Key;
use smallvec::SmallVec;

use super::*;

pub struct ReadSignal<T: 'static> {
    pub(crate) id: NodeId,
    root: &'static Root,
    #[cfg(debug_assertions)]
    created_at: &'static core::panic::Location<'static>,
    _phantom: PhantomData<T>,
}

pub struct Signal<T: 'static>(pub(crate) ReadSignal<T>);

#[cfg_attr(debug_assertions, track_caller)]
pub fn use_state<T>(initial: T) -> (ReadSignal<T>, impl Fn(T)) {
    let signal = use_signal(initial);
    let setter = move |value| signal.set(value);
    (signal.0, setter)
}

#[cfg_attr(debug_assertions, track_caller)]
pub fn use_signal<T>(value: T) -> Signal<T> {
    let signal = create_empty_signal();
    signal.get_mut().value = Some(Box::new(value));
    signal
}

#[cfg_attr(debug_assertions, track_caller)]
pub(crate) fn create_empty_signal<T>() -> Signal<T> {
    let root = Root::global();
    let id = root.nodes.borrow_mut().insert(ReactiveNode {
        value: None,
        callback: None,
        children: Vec::new(),
        parent: root.current_node.get(),
        dependents: Vec::new(),
        dependencies: SmallVec::new(),
        cleanups: Vec::new(),
        context: Vec::new(),
        state: NodeState::Clean,
        mark: Mark::None,
        #[cfg(debug_assertions)]
        created_at: core::panic::Location::caller(),
    });
    let current_node = root.current_node.get();
    if !current_node.is_null() {
        root.nodes.borrow_mut()[current_node].children.push(id);
    }

    Signal(ReadSignal {
        id,
        root,
        #[cfg(debug_assertions)]
        created_at: core::panic::Location::caller(),
        _phantom: PhantomData,
    })
}

impl<T> ReadSignal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    pub(crate) fn get_ref(self) -> Ref<'static, ReactiveNode> {
        Ref::map(
            self.root
                .nodes
                .try_borrow()
                .expect("cannot read signal while updating"),
            |nodes| match nodes.get(self.id) {
                Some(node) => node,
                None => panic!("{}", self.get_disposed_panic_message()),
            },
        )
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub(crate) fn get_mut(self) -> RefMut<'static, ReactiveNode> {
        RefMut::map(
            self.root
                .nodes
                .try_borrow_mut()
                .expect("cannot update signal while reading"),
            |nodes| match nodes.get_mut(self.id) {
                Some(node) => node,
                None => panic!("{}", self.get_disposed_panic_message()),
            },
        )
    }

    pub fn is_alive(self) -> bool {
        self.root.nodes.borrow().get(self.id).is_some()
    }

    pub fn dispose(self) {
        NodeHandle(self.id, self.root).dispose();
    }

    fn get_disposed_panic_message(self) -> String {
        #[cfg(not(debug_assertions))]
        return "signal was disposed".to_string();

        #[cfg(debug_assertions)]
        return format!("signal was disposed. Created at {}", self.created_at);
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn get_untracked(self) -> T
    where
        T: Copy,
    {
        self.with_untracked(|value| *value)
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn get_clone_untracked(self) -> T
    where
        T: Clone,
    {
        self.with_untracked(Clone::clone)
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn get(self) -> T
    where
        T: Copy,
    {
        self.track();
        self.get_untracked()
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn get_clone(self) -> T
    where
        T: Clone,
    {
        self.track();
        self.get_clone_untracked()
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn with_untracked<U>(self, f: impl FnOnce(&T) -> U) -> U {
        let node = self.get_ref();
        let value = node
            .value
            .as_ref()
            .expect("cannot read signal while updating");
        let ret = f(value.downcast_ref().expect("wrong signal type"));
        ret
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn with<U>(self, f: impl FnOnce(&T) -> U) -> U {
        self.track();
        self.with_untracked(f)
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn map<U>(self, mut f: impl FnMut(&T) -> U + 'static) -> ReadSignal<U> {
        use_memo(move || self.with(&mut f))
    }

    pub fn track(self) {
        if let Some(tracker) = &mut *self.root.tracker.borrow_mut() {
            tracker.dependencies.push(self.id);
        }
    }
}

impl<T> Signal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn set_silent(self, new: T) {
        self.replace_silent(new);
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn set(self, new: T) {
        self.replace(new);
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn replace_silent(self, new: T) -> T {
        self.update_silent(|val| core::mem::replace(val, new))
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn replace(self, new: T) -> T {
        self.update(|val| core::mem::replace(val, new))
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn take_silent(self) -> T
    where
        T: Default,
    {
        self.replace_silent(T::default())
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn take(self) -> T
    where
        T: Default,
    {
        self.replace(T::default())
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn update_silent<U>(self, f: impl FnOnce(&mut T) -> U) -> U {
        let mut value = self
            .get_mut()
            .value
            .take()
            .expect("cannot update signal while reading");
        let ret = f(value.downcast_mut().expect("wrong signal type"));
        self.get_mut().value = Some(value);
        ret
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn update<U>(self, f: impl FnOnce(&mut T) -> U) -> U {
        let ret = self.update_silent(f);
        self.0.root.propagate_updates(self.0.id);
        ret
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn set_fn_silent(self, f: impl FnOnce(&T) -> T) {
        self.update_silent(move |val| *val = f(val));
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn set_fn(self, f: impl FnOnce(&T) -> T) {
        self.update(move |val| *val = f(val));
    }

    pub fn split(self) -> (ReadSignal<T>, impl Fn(T) -> T) {
        (*self, move |value| self.replace(value))
    }
}

impl<T> Clone for ReadSignal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ReadSignal<T> {}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Signal<T> {}

impl<T: Default> Default for ReadSignal<T> {
    fn default() -> Self {
        *use_signal(Default::default())
    }
}
impl<T: Default> Default for Signal<T> {
    fn default() -> Self {
        use_signal(Default::default())
    }
}

impl<T: PartialEq> PartialEq for ReadSignal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.with(|value| other.with(|other| value == other))
    }
}
impl<T: Eq> Eq for ReadSignal<T> {}
impl<T: PartialOrd> PartialOrd for ReadSignal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.with(|value| other.with(|other| value.partial_cmp(other)))
    }
}
impl<T: Ord> Ord for ReadSignal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.with(|value| other.with(|other| value.cmp(other)))
    }
}
impl<T: Hash> Hash for ReadSignal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.with(|value| value.hash(state))
    }
}

impl<T: PartialEq> PartialEq for Signal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn eq(&self, other: &Self) -> bool {
        self.with(|value| other.with(|other| value == other))
    }
}
impl<T: Eq> Eq for Signal<T> {}
impl<T: PartialOrd> PartialOrd for Signal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.with(|value| other.with(|other| value.partial_cmp(other)))
    }
}
impl<T: Ord> Ord for Signal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.with(|value| other.with(|other| value.cmp(other)))
    }
}
impl<T: Hash> Hash for Signal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.with(|value| value.hash(state))
    }
}

impl<T> Deref for Signal<T> {
    type Target = ReadSignal<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: fmt::Debug> fmt::Debug for ReadSignal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.with(|value| value.fmt(f))
    }
}
impl<T: fmt::Debug> fmt::Debug for Signal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.with(|value| value.fmt(f))
    }
}

impl<T: fmt::Display> fmt::Display for ReadSignal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.with(|value| value.fmt(f))
    }
}
impl<T: fmt::Display> fmt::Display for Signal<T> {
    #[cfg_attr(debug_assertions, track_caller)]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.with(|value| value.fmt(f))
    }
}

#[cfg(feature = "serde")]
impl<T: serde::Serialize> serde::Serialize for ReadSignal<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.with(|value| value.serialize(serializer))
    }
}
#[cfg(feature = "serde")]
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for ReadSignal<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(*use_signal(T::deserialize(deserializer)?))
    }
}
#[cfg(feature = "serde")]
impl<T: serde::Serialize> serde::Serialize for Signal<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.with(|value| value.serialize(serializer))
    }
}
#[cfg(feature = "serde")]
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Signal<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(use_signal(T::deserialize(deserializer)?))
    }
}

impl<T: AddAssign<Rhs>, Rhs> AddAssign<Rhs> for Signal<T> {
    fn add_assign(&mut self, rhs: Rhs) {
        self.update(|this| *this += rhs);
    }
}
impl<T: SubAssign<Rhs>, Rhs> SubAssign<Rhs> for Signal<T> {
    fn sub_assign(&mut self, rhs: Rhs) {
        self.update(|this| *this -= rhs);
    }
}
impl<T: MulAssign<Rhs>, Rhs> MulAssign<Rhs> for Signal<T> {
    fn mul_assign(&mut self, rhs: Rhs) {
        self.update(|this| *this *= rhs);
    }
}
impl<T: DivAssign<Rhs>, Rhs> DivAssign<Rhs> for Signal<T> {
    fn div_assign(&mut self, rhs: Rhs) {
        self.update(|this| *this /= rhs);
    }
}
impl<T: RemAssign<Rhs>, Rhs> RemAssign<Rhs> for Signal<T> {
    fn rem_assign(&mut self, rhs: Rhs) {
        self.update(|this| *this %= rhs);
    }
}
