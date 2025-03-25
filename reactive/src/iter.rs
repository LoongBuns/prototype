use core::cell::RefCell;
use core::hash::Hash;

use alloc::rc::Rc;
use alloc::vec::Vec;

use hashbrown::HashMap;

use super::effect::untrack;
use super::state::StateHandle;
use super::{Scope, create_root};

pub fn map_indexed<T, U>(
    list: StateHandle<Vec<T>>,
    map_fn: impl Fn(&T) -> U + 'static,
) -> impl FnMut() -> Vec<U>
where
    T: PartialEq + Clone + 'static,
    U: Clone + 'static,
{
    let mut previous_items = Rc::new(Vec::new());
    let mapped_items = Rc::new(RefCell::new(Vec::new()));
    let mut scopes = Vec::new();

    move || {
        let items = list.get_tracked(); // Subscribe to list.
        untrack(|| {
            if items.is_empty() {
                // Fast path for removing all items.
                scopes = Vec::new();
                previous_items = Rc::new(Vec::new());
                *mapped_items.borrow_mut() = Vec::new();
            } else {
                // Pre-allocate space needed
                if items.len() > previous_items.len() {
                    let count = items.len() - previous_items.len();
                    mapped_items.borrow_mut().reserve(count);
                    scopes.reserve(count);
                }

                for (i, item) in items.iter().enumerate() {
                    let previous_item = previous_items.get(i);

                    if previous_item.is_none() {
                        let new_scope = create_root(|| {
                            mapped_items.borrow_mut().push(map_fn(item));
                        });
                        scopes.push(new_scope);
                    } else if previous_item != Some(item) {
                        let new_scope = create_root(|| {
                            mapped_items.borrow_mut()[i] = map_fn(item);
                        });
                        scopes[i] = new_scope;
                    }
                }

                if items.len() < previous_items.len() {
                    for _ in items.len()..previous_items.len() {
                        scopes.pop();
                    }
                }

                // In case the new set is shorter than the old, set the length of the mapped array.
                mapped_items.borrow_mut().truncate(items.len());
                scopes.truncate(items.len());

                // save a copy of the mapped items for the next update.
                previous_items = Rc::clone(&items);
                debug_assert!(
                    [
                        previous_items.len(),
                        mapped_items.borrow().len(),
                        scopes.len()
                    ]
                    .iter()
                    .all(|l| *l == items.len())
                );
            }

            mapped_items.borrow().clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use core::cell::Cell;

    use crate::*;

    #[test]
    fn indexed() {
        let a = StateHandle::new(vec![1, 2, 3]);
        let mut mapped = map_indexed(a.clone(), |x| *x * 2);
        assert_eq!(mapped(), vec![2, 4, 6]);

        a.set(vec![1, 2, 3, 4]);
        assert_eq!(mapped(), vec![2, 4, 6, 8]);

        a.set(vec![2, 2, 3, 4]);
        assert_eq!(mapped(), vec![4, 4, 6, 8]);
    }

    #[test]
    fn test_indexed_clear() {
        let a = StateHandle::new(vec![1, 2, 3]);
        let mut mapped = map_indexed(a.clone(), |x| *x * 2);

        a.set(Vec::new());
        assert_eq!(mapped(), Vec::<i32>::new());
    }

    #[test]
    fn test_indexed_react() {
        let a = StateHandle::new(vec![1, 2, 3]);
        let mut mapped = map_indexed(a.clone(), |x| *x * 2);

        let counter = StateHandle::new(0);
        create_effect({
            let counter = counter.clone();
            move || {
                counter.set(*counter.get() + 1);
                mapped();
            }
        });

        assert_eq!(*counter.get(), 1);
        a.set(vec![1, 2, 3, 4]);
        assert_eq!(*counter.get(), 2);
    }

    #[test]
    fn test_indexed_use_previous_computation() {
        let a = StateHandle::new(vec![1, 2, 3]);
        let counter = Rc::new(Cell::new(0));
        let mut mapped = map_indexed(a.clone(), {
            let counter = Rc::clone(&counter);
            move |_| {
                counter.set(counter.get() + 1);
                counter.get()
            }
        });

        assert_eq!(mapped(), vec![1, 2, 3]);

        a.set(vec![1, 2]);
        assert_eq!(mapped(), vec![1, 2]);

        a.set(vec![1, 2, 4]);
        assert_eq!(mapped(), vec![1, 2, 4]);

        a.set(vec![1, 3, 4]);
        assert_eq!(mapped(), vec![1, 5, 4]);
    }
}
