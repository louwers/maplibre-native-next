// Ports std::sort from libstdc++ (GCC 16) bits/stl_algo.h: `__sort`,
// `__introsort_loop`, `__unguarded_partition_pivot`, `__move_median_to_first`,
// `__unguarded_partition`, `__partial_sort`, `__heap_select`,
// `__final_insertion_sort`, `__insertion_sort`, `__unguarded_insertion_sort`,
// `__unguarded_linear_insert`.

use crate::heap::{adjust_heap_range, make_heap_range, sort_heap_range};

/// `_S_threshold`
const S_THRESHOLD: usize = 16;

/// `std::sort(first, last)` with `operator<`.
pub fn sort<T: Copy + PartialOrd>(v: &mut [T]) {
    sort_by(v, |a, b| a < b);
}

/// `std::sort(first, last, comp)` (`comp` is the C++ "less" predicate).
pub fn sort_by<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut comp: F) {
    let n = v.len();
    if n != 0 {
        // `std::__lg(n) * 2`
        let lg = (usize::BITS - 1 - n.leading_zeros()) as usize;
        introsort_loop(v, 0, n, lg * 2, &mut comp);
        final_insertion_sort(v, 0, n, &mut comp);
    }
}

fn introsort_loop<T: Copy, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mut last: usize,
    mut depth_limit: usize,
    comp: &mut F,
) {
    while last - first > S_THRESHOLD {
        if depth_limit == 0 {
            partial_sort(v, first, last, last, comp);
            return;
        }
        depth_limit -= 1;
        let cut = unguarded_partition_pivot(v, first, last, comp);
        introsort_loop(v, cut, last, depth_limit, comp);
        last = cut;
    }
}

fn unguarded_partition_pivot<T: Copy, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    comp: &mut F,
) -> usize {
    let mid = first + (last - first) / 2;
    move_median_to_first(v, first, first + 1, mid, last - 1, comp);
    unguarded_partition(v, first + 1, last, first, comp)
}

/// Swaps the median of `v[a]`, `v[b]` and `v[c]` under `comp` to `v[result]`.
fn move_median_to_first<T: Copy, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    result: usize,
    a: usize,
    b: usize,
    c: usize,
    comp: &mut F,
) {
    if comp(&v[a], &v[b]) {
        if comp(&v[b], &v[c]) {
            v.swap(result, b);
        } else if comp(&v[a], &v[c]) {
            v.swap(result, c);
        } else {
            v.swap(result, a);
        }
    } else if comp(&v[a], &v[c]) {
        v.swap(result, a);
    } else if comp(&v[b], &v[c]) {
        v.swap(result, c);
    } else {
        v.swap(result, b);
    }
}

fn unguarded_partition<T: Copy, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    mut first: usize,
    mut last: usize,
    pivot: usize,
    comp: &mut F,
) -> usize {
    loop {
        while comp(&v[first], &v[pivot]) {
            first += 1;
        }
        last = last.wrapping_sub(1);
        while comp(&v[pivot], &v[last]) {
            last = last.wrapping_sub(1);
        }
        if first >= last {
            return first;
        }
        v.swap(first, last);
        first += 1;
    }
}

/// `__partial_sort(first, middle, last)`.
fn partial_sort<T: Copy, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    middle: usize,
    last: usize,
    comp: &mut F,
) {
    heap_select(v, first, middle, last, comp);
    sort_heap_range(v, first, middle, comp);
}

/// `__heap_select(first, middle, last)`.
fn heap_select<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], first: usize, middle: usize, last: usize, comp: &mut F) {
    make_heap_range(v, first, middle, comp);
    for i in middle..last {
        if comp(&v[i], &v[first]) {
            // `__pop_heap(first, middle, i)`
            let value = v[i];
            v[i] = v[first];
            adjust_heap_range(v, first, 0, (middle - first) as isize, value, comp);
        }
    }
}

fn final_insertion_sort<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], first: usize, last: usize, comp: &mut F) {
    if last - first > S_THRESHOLD {
        insertion_sort(v, first, first + S_THRESHOLD, comp);
        // `__unguarded_insertion_sort(first + threshold, last)`
        for i in first + S_THRESHOLD..last {
            unguarded_linear_insert(v, i, comp);
        }
    } else {
        insertion_sort(v, first, last, comp);
    }
}

/// `__insertion_sort(first, last)`; also used by `std::stable_sort`.
pub(crate) fn insertion_sort<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], first: usize, last: usize, comp: &mut F) {
    if first == last {
        return;
    }
    for i in first + 1..last {
        if comp(&v[i], &v[first]) {
            let val = v[i];
            v.copy_within(first..i, first + 1);
            v[first] = val;
        } else {
            unguarded_linear_insert(v, i, comp);
        }
    }
}

fn unguarded_linear_insert<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut last: usize, comp: &mut F) {
    let val = v[last];
    let mut next = last.wrapping_sub(1);
    while comp(&val, &v[next]) {
        v[last] = v[next];
        last = next;
        next = next.wrapping_sub(1);
    }
    v[last] = val;
}
