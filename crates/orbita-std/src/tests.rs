#[cfg(test)]
mod tests {
    use crate::prelude::*;

    #[test]
    fn vec_push_sort_dedup() {
        let mut v = vec![3, 1, 2, 1, 3];
        v.sort();
        v.dedup();
        assert_eq!(v, vec![1, 2, 3]);
    }

    #[test]
    fn string_format_and_concat() {
        let s = format!("orbita-{}-{}", 1, true);
        assert_eq!(s, "orbita-1-true");
        let joined = [String::from("a"), String::from("b")].join("-");
        assert_eq!(joined, "a-b");
    }

    #[test]
    fn btree_map_ordered_insert_remove() {
        let mut map = BTreeMap::new();
        map.insert(3, "c");
        map.insert(1, "a");
        map.insert(2, "b");
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![1, 2, 3]);
        assert_eq!(map.remove(&2), Some("b"));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn btreeset_union_contains() {
        let a: BTreeSet<u32> = [1, 2, 3].into_iter().collect();
        let b: BTreeSet<u32> = [3, 4].into_iter().collect();
        let union: BTreeSet<u32> = a.union(&b).copied().collect();
        assert_eq!(union.len(), 4);
        assert!(union.contains(&4));
    }

    #[test]
    fn vecdeque_front_back() {
        let mut dq = VecDeque::new();
        dq.push_back(2);
        dq.push_front(1);
        dq.push_back(3);
        assert_eq!(dq.pop_front(), Some(1));
        assert_eq!(dq.pop_back(), Some(3));
        assert_eq!(dq.len(), 1);
    }

    #[test]
    fn rc_arc_sharing() {
        let rc = Rc::new(41);
        let rc2 = Rc::clone(&rc);
        assert_eq!(Rc::strong_count(&rc), 2);
        assert_eq!(*rc2, 41);

        let arc = Arc::new(vec![1, 2, 3]);
        let arc2 = Arc::clone(&arc);
        assert_eq!(Arc::strong_count(&arc), 2);
        assert_eq!(arc2.len(), 3);
    }

    #[test]
    fn binary_heap_ordering() {
        let mut heap = BinaryHeap::new();
        heap.push(3);
        heap.push(1);
        heap.push(5);
        heap.push(2);
        assert_eq!(heap.pop(), Some(5));
        assert_eq!(heap.pop(), Some(3));
    }

    #[test]
    fn boxed_slice_helper_still_works() {
        let b = crate::boxed_slice(16, 0xAB);
        assert_eq!(b.len(), 16);
        assert!(b.iter().all(|&x| x == 0xAB));
    }

    #[test]
    fn duration_helpers() {
        use crate::time::{duration_millis, duration_micros};
        assert_eq!(duration_millis(1500).as_secs(), 1);
        assert_eq!(duration_micros(1_500_000).as_secs(), 1);
    }

    #[test]
    fn cstring_ffi_roundtrip() {
        let c = CString::new("orbita").unwrap();
        let bytes = c.as_bytes_with_nul();
        assert_eq!(bytes, b"orbita\0");
    }

    #[test]
    fn println_macro_does_not_panic() {
        // Console backend is inert until the platform console is
        // initialized; on the host it must simply not crash.
        println!("hello {} from orbita std", 42);
    }
}
