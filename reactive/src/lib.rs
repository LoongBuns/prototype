#![no_std]

#[macro_use]
extern crate alloc;

mod context;
mod effects;
mod iter;
mod maybe_dyn;
mod memos;
mod node;
mod root;
mod state;
mod track;

use core::fmt;

use alloc::boxed::Box;

pub use context::*;
pub use effects::*;
pub use iter::*;
pub use maybe_dyn::*;
pub use memos::*;
pub use node::*;
pub use root::*;
pub use state::*;
pub use track::*;

#[doc(hidden)]
pub fn component_scope<T>(f: impl FnOnce() -> T) -> T {
    untrack(f)
}

pub trait Props {
    type Builder;

    fn builder() -> Self::Builder;
}

impl Props for () {
    type Builder = UnitBuilder;
    fn builder() -> Self::Builder {
        UnitBuilder
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub struct UnitBuilder;

impl UnitBuilder {
    pub fn build(self) {}
}

pub trait Component<T: Props, V, S> {
    fn create(self, props: T) -> V;
}
impl<F, T: Props, V> Component<T, V, ((),)> for F
where
    F: FnOnce(T) -> V,
{
    fn create(self, props: T) -> V {
        self(props)
    }
}
impl<F, V> Component<(), V, ()> for F
where
    F: FnOnce() -> V,
{
    fn create(self, _props: ()) -> V {
        self()
    }
}

#[doc(hidden)]
pub fn element_like_component_builder<T: Props, V, S>(_f: &impl Component<T, V, S>) -> T::Builder {
    T::builder()
}

pub struct Children<V> {
    f: Box<dyn FnOnce() -> V>,
}
impl<V> fmt::Debug for Children<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Children").finish()
    }
}

impl<F, V> From<F> for Children<V>
where
    F: FnOnce() -> V + 'static,
{
    fn from(f: F) -> Self {
        Self { f: Box::new(f) }
    }
}

impl<V: Default + 'static> Default for Children<V> {
    fn default() -> Self {
        Self {
            f: Box::new(V::default),
        }
    }
}

impl<V> Children<V> {
    pub fn call(self) -> V {
        (self.f)()
    }

    pub fn new(f: impl FnOnce() -> V + 'static) -> Self {
        Self { f: Box::new(f) }
    }
}
