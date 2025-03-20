#[macro_use]
extern crate alloc;

mod effect;
mod iter;
mod state;

pub use effect::*;
pub use iter::*;
pub use state::*;

#[derive(Debug, Default, Clone, PartialOrd, PartialEq)]
#[repr(C)]
pub enum FiberValue {
    #[default]
    Void,
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    List(*mut [FiberValue]),
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
