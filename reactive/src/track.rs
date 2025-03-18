use super::*;

pub trait Trackable {
    fn track_data(&self);
}

impl<T> Trackable for Signal<T> {
    fn track_data(&self) {
        self.track();
    }
}

impl<T> Trackable for ReadSignal<T> {
    fn track_data(&self) {
        self.track();
    }
}

impl<T: Into<Self>> Trackable for MaybeDyn<T> {
    fn track_data(&self) {
        match self {
            MaybeDyn::Static(_) => {}
            MaybeDyn::Signal(signal) => signal.track(),
            MaybeDyn::Derived(f) => f().track_data(),
        }
    }
}

macro_rules! impl_trackable_deps_for_tuple {
    ($($T:tt),*) => {
        paste::paste! {
            impl<$($T,)*> Trackable for ($($T,)*)
            where
                $($T: Trackable,)*
            {
                fn track_data(&self) {
                    let ($([<$T:lower>],)*) = self;
                    $(
                        [<$T:lower>].track_data();
                    )*
                }
            }
        }
    }
}

impl_trackable_deps_for_tuple!(A);
impl_trackable_deps_for_tuple!(A, B);
impl_trackable_deps_for_tuple!(A, B, C);
impl_trackable_deps_for_tuple!(A, B, C, D);
impl_trackable_deps_for_tuple!(A, B, C, D, E);
impl_trackable_deps_for_tuple!(A, B, C, D, E, F);
impl_trackable_deps_for_tuple!(A, B, C, D, E, F, G);
impl_trackable_deps_for_tuple!(A, B, C, D, E, F, G, H);
impl_trackable_deps_for_tuple!(A, B, C, D, E, F, G, H, I);
impl_trackable_deps_for_tuple!(A, B, C, D, E, F, G, H, I, J);
impl_trackable_deps_for_tuple!(A, B, C, D, E, F, G, H, I, J, K);
impl_trackable_deps_for_tuple!(A, B, C, D, E, F, G, H, I, J, K, L);

pub fn on<T>(
    deps: impl Trackable + 'static,
    mut f: impl FnMut() -> T + 'static,
) -> impl FnMut() -> T + 'static {
    move || {
        deps.track_data();
        untrack(&mut f)
    }
}
