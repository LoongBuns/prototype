use core::mem;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::*;

pub fn map_keyed<T, K, U>(
    list: impl Into<MaybeDyn<Vec<T>>> + 'static,
    mut map_fn: impl FnMut(T) -> U + 'static,
    key_fn: impl Fn(&T) -> K + 'static,
) -> ReadSignal<Vec<U>>
where
    T: PartialEq + Clone + 'static,
    U: Clone,
    K: Ord,
{
    let list = list.into();
    let mut previous_items = Vec::new();
    let mut mapped_items: Vec<U> = Vec::new();
    let mut disposers: Vec<Option<NodeHandle>> = Vec::new();

    let list_clone = list.clone();
    let mut update_handler = move || {
        let items = list_clone.get_clone();
        if items.is_empty() {
            for disposer in mem::take(&mut disposers) {
                disposer.unwrap().dispose();
            }
            mapped_items = Vec::new();
        } else if previous_items.is_empty() {
            mapped_items.reserve(items.len());
            disposers.reserve(items.len());

            for new_item in items.iter().cloned() {
                let map_fn = &mut map_fn;
                let mapped = &mut mapped_items;
                let new_disposer = create_child_scope(move || mapped.push(map_fn(new_item)));
                disposers.push(Some(new_disposer));
            }
        } else {
            let (start, end, new_end) = {
                let min_len = usize::min(previous_items.len(), items.len());
                let start = previous_items
                    .iter()
                    .zip(items.iter())
                    .position(|(a, b)| a != b)
                    .unwrap_or(min_len);
                debug_assert!(
                    (previous_items.get(start).is_none() && items.get(start).is_none())
                        || (previous_items.get(start) != items.get(start)),
                    "start is the first index where items[start] != new_items[start]"
                );

                let mut end = previous_items.len();
                let mut new_end = items.len();
                while end > start
                    && new_end > start
                    && previous_items[end - 1] == items[new_end - 1]
                {
                    end -= 1;
                    new_end -= 1;
                }
                debug_assert!(
                    if end != 0 && new_end != 0 {
                        (end == previous_items.len() && new_end == items.len())
                            || (previous_items[end - 1] != items[new_end - 1])
                    } else {
                        true
                    },
                    "end and new_end are the last indexes where items[end - 1] != new_items[new_end - 1]"
                );

                (start, end, new_end)
            };

            let mut pending_mapped = vec![None; items.len()];
            let mut pending_disposers = vec![None; items.len()];

            for i in end..previous_items.len() {
                let new_idx = new_end + (i - end);
                pending_mapped[new_idx] = Some(mapped_items[i].clone());
                pending_disposers[new_idx] = disposers[i].take();
            }

            let mut indices = BTreeMap::new();
            let mut indices_next = vec![None; new_end - start];
            for j in (start..new_end).rev() {
                let key = key_fn(&items[j]);
                indices_next[j - start] = indices.get(&key).copied();
                indices.insert(key, j);
            }

            for i in start..end {
                let key = key_fn(&previous_items[i]);
                if let Some(j) = indices.get(&key).copied() {
                    pending_mapped[j] = Some(mapped_items[i].clone());
                    pending_disposers[j] = disposers[i].take();
                    indices_next[j - start].and_then(|j| indices.insert(key, j));
                } else {
                    disposers[i].take().unwrap().dispose();
                }
            }

            for j in start..items.len() {
                if matches!(pending_mapped.get(j), Some(Some(_))) {
                    if j >= mapped_items.len() {
                        debug_assert_eq!(mapped_items.len(), j);
                        mapped_items.push(pending_mapped[j].clone().unwrap());
                        disposers.push(pending_disposers[j].take());
                    } else {
                        mapped_items[j] = pending_mapped[j].clone().unwrap();
                        disposers[j] = pending_disposers[j].take();
                    }
                } else {
                    let mut result = None;
                    let item = items[j].clone();
                    let disposer = create_child_scope(|| result = Some(map_fn(item)));
                    if j < mapped_items.len() {
                        mapped_items[j] = result.unwrap();
                        disposers[j] = Some(disposer);
                    } else {
                        mapped_items.push(result.unwrap());
                        disposers.push(Some(disposer));
                    }
                }
            }
        }

        mapped_items.truncate(items.len());
        disposers.truncate(items.len());

        debug_assert!(
            [mapped_items.len(), disposers.len()]
                .iter()
                .all(|l| *l == items.len())
        );
        previous_items = items;

        mapped_items.clone()
    };
    let scope = use_current_scope();
    use_memo(on(list, move || scope.run_in(&mut update_handler)))
}

pub fn map_indexed<T, U>(
    list: impl Into<MaybeDyn<Vec<T>>> + 'static,
    mut map_fn: impl FnMut(T) -> U + 'static,
) -> ReadSignal<Vec<U>>
where
    T: PartialEq + Clone + 'static,
    U: Clone,
{
    let list = list.into();
    let mut previous_items = Vec::new();
    let mut mapped_items = Vec::new();
    let mut disposers: Vec<NodeHandle> = Vec::new();

    let list_clone = list.clone();
    let mut update_handler = move || {
        let items = list_clone.get_clone();
        if items.is_empty() {
            for disposer in mem::take(&mut disposers) {
                disposer.dispose();
            }
            previous_items = Vec::new();
            mapped_items = Vec::new();
        } else {
            if items.len() > previous_items.len() {
                let count = items.len() - previous_items.len();
                mapped_items.reserve(count);
                disposers.reserve(count);
            }

            for (i, item) in items.iter().cloned().enumerate() {
                if previous_items.get(i).is_none_or(|prev| prev != &item) {
                    let mut result = None;
                    let disposer = create_child_scope(|| result = Some(map_fn(item)));
                    if let Some(existing) = mapped_items.get_mut(i) {
                        *existing = result.unwrap();
                        let prev = mem::replace(&mut disposers[i], disposer);
                        prev.dispose();
                    } else {
                        mapped_items.push(result.unwrap());
                        disposers.push(disposer);
                    }
                }
            }

            if items.len() < previous_items.len() {
                for _ in items.len()..previous_items.len() {
                    disposers.pop().unwrap().dispose();
                }
            }

            mapped_items.truncate(items.len());

            debug_assert!(
                [mapped_items.len(), disposers.len()]
                    .iter()
                    .all(|l| *l == items.len())
            );
            previous_items = items;
        }

        mapped_items.clone()
    };
    let scope = use_current_scope();
    use_memo(on(list, move || scope.run_in(&mut update_handler)))
}
