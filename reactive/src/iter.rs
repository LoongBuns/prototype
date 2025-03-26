use core::hash::Hash;
use core::mem;

use alloc::rc::Rc;
use alloc::vec::Vec;

use hashbrown::HashMap;

use super::create_root;
use super::effect::untrack;
use super::state::StateHandle;

pub fn map_keyed<T, K, U>(
    list: StateHandle<Vec<T>>,
    map_fn: impl Fn(&T) -> U + 'static,
    key_fn: impl Fn(&T) -> K + 'static,
) -> impl FnMut() -> Vec<U>
where
    T: PartialEq + Clone + 'static,
    K: Eq + Hash,
    U: Clone + 'static,
{
    let mut previous_items = Rc::new(Vec::new());
    let mut mapped_items = Vec::new();
    let mut mapped_items_tmp = Vec::new();
    let mut scopes = Vec::new();
    let mut scopes_tmp = Vec::new();

    move || {
        let items = list.get();
        untrack(|| {
            if items.is_empty() {
                scopes = Vec::new();
                mapped_items = Vec::new();
            } else if previous_items.is_empty() {
                for item in items.iter() {
                    let scope = create_root(|| mapped_items.push(map_fn(item)));
                    scopes.push(Some(Rc::new(scope)));
                }
            } else {
                mapped_items_tmp.clear();
                mapped_items_tmp.resize(items.len(), None);
                scopes_tmp.clear();
                scopes_tmp.resize_with(items.len(), || None);

                let min_len = usize::min(previous_items.len(), items.len());
                let start = previous_items
                    .iter()
                    .zip(items.iter())
                    .position(|(a, b)| a != b)
                    .unwrap_or(min_len);
                debug_assert!(
                    (previous_items.get(start).is_none() && items.get(start).is_none())
                        || (previous_items.get(start) != items.get(start))
                );

                let mut end = previous_items.len();
                let mut new_end = items.len();
                while end > start
                    && new_end > start
                    && previous_items[end - 1] == items[new_end - 1]
                {
                    end -= 1;
                    new_end -= 1;
                    mapped_items_tmp[new_end] = Some(mapped_items[end].clone());
                    scopes_tmp[new_end] = scopes[end].take();
                }
                debug_assert!(if end != 0 && new_end != 0 {
                    (end == previous_items.len() && new_end == items.len())
                        || (previous_items[end - 1] != items[new_end - 1])
                } else {
                    true
                });

                let mut indices = HashMap::with_capacity(new_end - start);
                let mut indices_next = vec![None; new_end - start];
                for j in (start..new_end).rev() {
                    let key = key_fn(&items[j]);
                    indices_next[j - start] = indices.get(&key).copied();
                    indices.insert(key, j);
                }

                for i in start..end {
                    let key = key_fn(&previous_items[i]);
                    if let Some(j) = indices.get(&key).copied() {
                        mapped_items_tmp[j] = Some(mapped_items[i].clone());
                        scopes_tmp[j] = scopes[i].take();
                        indices_next[j - start].and_then(|j| indices.insert(key, j));
                    } else {
                        drop(scopes[i].take());
                    }
                }

                for j in start..items.len() {
                    if matches!(mapped_items_tmp.get(j), Some(Some(_))) {
                        if j >= mapped_items.len() {
                            debug_assert_eq!(mapped_items.len(), j);
                            mapped_items.push(mapped_items_tmp[j].clone().unwrap());
                            scopes.push(scopes_tmp[j].take());
                        } else {
                            mapped_items[j] = mapped_items_tmp[j].clone().unwrap();
                            scopes[j] = scopes_tmp[j].take();
                        }
                    } else {
                        let mut mapped = None;
                        let scope = create_root(|| mapped = Some(map_fn(&items[j])));
                        if j < mapped_items.len() {
                            mapped_items[j] = mapped.unwrap();
                            scopes[j] = Some(Rc::new(scope));
                        } else {
                            mapped_items.push(mapped.unwrap());
                            scopes.push(Some(Rc::new(scope)));
                        }
                    }
                }
            }

            mapped_items.truncate(items.len());
            scopes.truncate(items.len());

            previous_items = Rc::clone(&items);
            debug_assert!(
                [previous_items.len(), mapped_items.len(), scopes.len()]
                    .iter()
                    .all(|l| items.len() == *l)
            );

            mapped_items.clone()
        })
    }
}

pub fn map_indexed<T, U>(
    list: StateHandle<Vec<T>>,
    map_fn: impl Fn(&T) -> U + 'static,
) -> impl FnMut() -> Vec<U>
where
    T: PartialEq + Clone + 'static,
    U: Clone + 'static,
{
    let mut previous_items = Rc::new(Vec::new());
    let mut mapped_items = Vec::new();
    let mut scopes = Vec::new();

    move || {
        let items = list.get_tracked(); // Subscribe to list.
        untrack(|| {
            if items.is_empty() {
                // Fast path for removing all items.
                scopes = Vec::new();
                previous_items = Rc::new(Vec::new());
                mapped_items = Vec::new();
            } else {
                // Pre-allocate space needed
                if items.len() > previous_items.len() {
                    let count = items.len() - previous_items.len();
                    mapped_items.reserve(count);
                    scopes.reserve(count);
                }

                for (i, item) in items.iter().enumerate() {
                    if previous_items.get(i).is_none_or(|prev| prev != item) {
                        let mut mapped = None;
                        let scope = create_root(|| mapped = Some(map_fn(item)));
                        if let Some(existing) = mapped_items.get_mut(i) {
                            *existing = mapped.unwrap();
                            let prev = mem::replace(&mut scopes[i], scope);
                            drop(prev);
                        } else {
                            mapped_items.push(mapped.unwrap());
                            scopes.push(scope);
                        }
                    }
                }

                if items.len() < previous_items.len() {
                    for _ in items.len()..previous_items.len() {
                        drop(scopes.pop());
                    }
                }

                // In case the new set is shorter than the old, set the length of the mapped array.
                mapped_items.truncate(items.len());
                scopes.truncate(items.len());

                // save a copy of the mapped items for the next update.
                previous_items = Rc::clone(&items);
                debug_assert!(
                    [previous_items.len(), mapped_items.len(), scopes.len()]
                        .iter()
                        .all(|l| items.len() == *l)
                );
            }

            mapped_items.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;

    use alloc::rc::Rc;
    use alloc::vec::Vec;

    use crate::*;

    #[test]
    fn test_keyed() {
        let a = StateHandle::new(vec![1, 2, 3]);
        let mut mapped = map_keyed(a.clone(), |x| *x * 2, |x| *x);
        assert_eq!(mapped(), vec![2, 4, 6]);

        a.set(vec![1, 2, 3, 4]);
        assert_eq!(mapped(), vec![2, 4, 6, 8]);

        a.set(vec![2, 2, 3, 4]);
        assert_eq!(mapped(), vec![4, 4, 6, 8]);
    }

    #[test]
    fn test_keyed_recompute_everything() {
        let a = StateHandle::new(vec![1, 2, 3]);
        let mut mapped = map_keyed(a.clone(), |x| *x * 2, |x| *x);
        assert_eq!(mapped(), vec![2, 4, 6]);

        a.set(vec![4, 5, 6]);
        assert_eq!(mapped(), vec![8, 10, 12]);
    }

    #[test]
    fn test_keyed_clear() {
        let a = StateHandle::new(vec![1, 2, 3]);
        let mut mapped = map_keyed(a.clone(), |x| *x * 2, |x| *x);

        a.set(Vec::new());
        assert_eq!(mapped(), Vec::<i32>::new());
    }

    #[test]
    fn test_keyed_use_previous_computation() {
        let a = StateHandle::new(vec![1, 2, 3]);
        let counter = Rc::new(Cell::new(0));
        let mut mapped = map_keyed(
            a.clone(),
            {
                let counter = Rc::clone(&counter);
                move |_| {
                    counter.set(counter.get() + 1);
                    counter.get()
                }
            },
            |x| *x,
        );
        assert_eq!(mapped(), vec![1, 2, 3]);

        a.set(vec![1, 2]);
        assert_eq!(mapped(), vec![1, 2]);

        a.set(vec![1, 2, 4]);
        assert_eq!(mapped(), vec![1, 2, 4]);

        a.set(vec![1, 2, 3, 4]);
        assert_eq!(mapped(), vec![1, 2, 5, 4]);
    }

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
