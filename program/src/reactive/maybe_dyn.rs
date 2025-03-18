use alloc::borrow::Cow;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use super::*;

#[derive(Clone)]
pub enum MaybeDyn<T>
where
    T: Into<Self> + 'static,
{
    Static(T),
    Signal(ReadSignal<T>),
    Derived(Rc<dyn Fn() -> Self>),
}

impl<T: Into<Self> + 'static> MaybeDyn<T> {
    pub fn evaluate(self) -> T
    where
        T: Clone,
    {
        match self {
            Self::Static(value) => value,
            Self::Signal(signal) => signal.get_clone(),
            Self::Derived(f) => f().evaluate(),
        }
    }

    pub fn get(&self) -> T
    where
        T: Copy,
    {
        match self {
            Self::Static(value) => *value,
            Self::Signal(value) => value.get(),
            Self::Derived(f) => f().evaluate(),
        }
    }

    pub fn get_clone(&self) -> T
    where
        T: Clone,
    {
        match self {
            Self::Static(value) => value.clone(),
            Self::Signal(value) => value.get_clone(),
            Self::Derived(f) => f().evaluate(),
        }
    }

    pub fn track(&self) {
        match self {
            Self::Static(_) => {}
            Self::Signal(signal) => signal.track(),
            Self::Derived(f) => f().track(),
        }
    }

    pub fn as_static(&self) -> Option<&T> {
        match self {
            Self::Static(value) => Some(value),
            _ => None,
        }
    }
}

impl<T: Into<Self>, U: Into<MaybeDyn<T>> + Clone> From<ReadSignal<U>> for MaybeDyn<T> {
    fn from(val: ReadSignal<U>) -> Self {
        if let Some(val) =
            (&mut Some(val) as &mut dyn core::any::Any).downcast_mut::<Option<ReadSignal<T>>>()
        {
            MaybeDyn::Signal(val.unwrap())
        } else {
            MaybeDyn::Derived(Rc::new(move || val.get_clone().into()))
        }
    }
}

impl<T: Into<Self>, U: Into<MaybeDyn<T>> + Clone> From<Signal<U>> for MaybeDyn<T> {
    fn from(val: Signal<U>) -> Self {
        Self::from(*val)
    }
}

impl<F, U, T: Into<Self>> From<F> for MaybeDyn<T>
where
    F: Fn() -> U + 'static,
    U: Into<MaybeDyn<T>>,
{
    fn from(f: F) -> Self {
        MaybeDyn::Derived(Rc::new(move || f().into()))
    }
}

#[macro_export]
macro_rules! impl_into_maybe_dyn {
    ($ty:ty $(; $($from:ty),*)?) => {
        impl From<$ty> for $crate::reactive::MaybeDyn<$ty> {
            fn from(val: $ty) -> Self {
                MaybeDyn::Static(val)
            }
        }

        $crate::impl_into_maybe_dyn_with_convert!($ty; Into::into $(; $($from),*)?);
    };
}

#[macro_export]
macro_rules! impl_into_maybe_dyn_with_convert {
    ($ty:ty; $convert:expr $(; $($from:ty),*)?) => {
        $(
            $(
                impl From<$from> for $crate::reactive::MaybeDyn<$ty> {
                    fn from(val: $from) -> Self {
                        MaybeDyn::Static($convert(val))
                    }
                }
            )*
        )?
    };
}

impl_into_maybe_dyn!(Cow<'static, str>; &'static str, String);
impl_into_maybe_dyn_with_convert!(
    Option<Cow<'static, str>>; |x| Some(Into::into(x));
    Cow<'static, str>, &'static str, String
);
impl_into_maybe_dyn_with_convert!(
    Option<Cow<'static, str>>; |x| Option::map(x, Into::into);
    Option<&'static str>, Option<String>
);

impl_into_maybe_dyn!(bool);

impl_into_maybe_dyn!(f32);
impl_into_maybe_dyn!(f64);

impl_into_maybe_dyn!(i8);
impl_into_maybe_dyn!(i16);
impl_into_maybe_dyn!(i32);
impl_into_maybe_dyn!(i64);
impl_into_maybe_dyn!(i128);
impl_into_maybe_dyn!(isize);
impl_into_maybe_dyn!(u8);
impl_into_maybe_dyn!(u16);
impl_into_maybe_dyn!(u32);
impl_into_maybe_dyn!(u64);
impl_into_maybe_dyn!(u128);
impl_into_maybe_dyn!(usize);

impl<T> From<Option<T>> for MaybeDyn<Option<T>> {
    fn from(val: Option<T>) -> Self {
        MaybeDyn::Static(val)
    }
}

impl<T> From<Vec<T>> for MaybeDyn<Vec<T>> {
    fn from(val: Vec<T>) -> Self {
        MaybeDyn::Static(val)
    }
}
