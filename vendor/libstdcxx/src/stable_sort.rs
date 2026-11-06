// Ports std::stable_sort from libstdc++ (GCC 16) bits/stl_algo.h:
// `__stable_sort`, `__stable_sort_adaptive`, `__merge_sort_with_buffer`,
// `__chunk_insertion_sort`, `__merge_sort_loop`, `__move_merge`,
// `__merge_adaptive`, `__move_merge_adaptive`,
// `__move_merge_adaptive_backward`.
//
// libstdc++ asks for a temporary buffer of `(n + 1) / 2` elements and, when
// it gets all of it (always, short of an allocation failure), sorts both
// halves with `__merge_sort_with_buffer` and merges them with
// `__merge_adaptive`. The buffer-less (`__inplace_stable_sort`) and
// partial-buffer (`__stable_sort_adaptive_resize`) fallbacks are not ported:
// a failed allocation aborts the Rust process instead.

use crate::sort::insertion_sort;

/// `_S_chunk_size`
const S_CHUNK_SIZE: usize = 7;

/// `std::stable_sort(first, last, comp)` (`comp` is the C++ "less"
/// predicate).
pub fn stable_sort_by<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut comp: F) {
    let n = v.len();
    if n == 0 {
        return;
    }
    let buffer_len = n.div_ceil(2);
    let mut buffer: Vec<T> = vec![v[0]; buffer_len];
    // `__stable_sort_adaptive(first, first + buf.size(), last, buf, comp)`
    let middle = buffer_len;
    merge_sort_with_buffer(&mut v[..middle], &mut buffer, &mut comp);
    merge_sort_with_buffer(&mut v[middle..], &mut buffer, &mut comp);
    merge_adaptive(v, middle, &mut buffer, &mut comp);
}

fn merge_sort_with_buffer<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], buffer: &mut [T], comp: &mut F) {
    let len = v.len();
    let mut step_size = S_CHUNK_SIZE;
    chunk_insertion_sort(v, step_size, comp);
    while step_size < len {
        merge_sort_loop(v, &mut buffer[..len], step_size, comp);
        step_size *= 2;
        merge_sort_loop(&buffer[..len], v, step_size, comp);
        step_size *= 2;
    }
}

fn chunk_insertion_sort<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], chunk_size: usize, comp: &mut F) {
    let last = v.len();
    let mut first = 0;
    while last - first >= chunk_size {
        insertion_sort(v, first, first + chunk_size, comp);
        first += chunk_size;
    }
    insertion_sort(v, first, last, comp);
}

/// `__merge_sort_loop(src.begin(), src.end(), dst.begin(), step_size)`.
fn merge_sort_loop<T: Copy, F: FnMut(&T, &T) -> bool>(src: &[T], dst: &mut [T], mut step_size: usize, comp: &mut F) {
    let two_step = 2 * step_size;
    let last = src.len();
    let mut first = 0;
    let mut result = 0;
    while last - first >= two_step {
        result =
            move_merge(&src[first..first + step_size], &src[first + step_size..first + two_step], dst, result, comp);
        first += two_step;
    }
    step_size = step_size.min(last - first);
    move_merge(&src[first..first + step_size], &src[first + step_size..last], dst, result, comp);
}

/// `__move_merge(a, b, dst + result)`; returns the new `result`.
fn move_merge<T: Copy, F: FnMut(&T, &T) -> bool>(
    a: &[T],
    b: &[T],
    dst: &mut [T],
    mut result: usize,
    comp: &mut F,
) -> usize {
    let (mut i, mut j) = (0, 0);
    while i != a.len() && j != b.len() {
        if comp(&b[j], &a[i]) {
            dst[result] = b[j];
            j += 1;
        } else {
            dst[result] = a[i];
            i += 1;
        }
        result += 1;
    }
    for x in &a[i..] {
        dst[result] = *x;
        result += 1;
    }
    for x in &b[j..] {
        dst[result] = *x;
        result += 1;
    }
    result
}

/// `__merge_adaptive(first, middle, last, len1, len2, buffer)` with a buffer
/// that holds the smaller half.
fn merge_adaptive<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], middle: usize, buffer: &mut [T], comp: &mut F) {
    let len1 = middle;
    let len2 = v.len() - middle;
    if len1 <= len2 {
        buffer[..len1].copy_from_slice(&v[..len1]);
        // `__move_merge_adaptive(buffer, buffer_end, middle, last, first)`
        let (mut i1, l1) = (0, len1);
        let (mut i2, l2) = (middle, v.len());
        let mut result = 0;
        while i1 != l1 && i2 != l2 {
            if comp(&v[i2], &buffer[i1]) {
                v[result] = v[i2];
                i2 += 1;
            } else {
                v[result] = buffer[i1];
                i1 += 1;
            }
            result += 1;
        }
        if i1 != l1 {
            v[result..result + (l1 - i1)].copy_from_slice(&buffer[i1..l1]);
        }
    } else {
        buffer[..len2].copy_from_slice(&v[middle..]);
        // `__move_merge_adaptive_backward(first, middle, buffer, buffer_end,
        // last)`
        let first1 = 0;
        let mut last1 = middle;
        let first2 = 0;
        let mut last2 = len2;
        let mut result = v.len();
        if first1 == last1 {
            v[result - (last2 - first2)..result].copy_from_slice(&buffer[first2..last2]);
            return;
        } else if first2 == last2 {
            return;
        }
        last1 -= 1;
        last2 -= 1;
        loop {
            if comp(&buffer[last2], &v[last1]) {
                result -= 1;
                v[result] = v[last1];
                if first1 == last1 {
                    let n = last2 + 1 - first2;
                    v[result - n..result].copy_from_slice(&buffer[first2..last2 + 1]);
                    return;
                }
                last1 -= 1;
            } else {
                result -= 1;
                v[result] = buffer[last2];
                if first2 == last2 {
                    return;
                }
                last2 -= 1;
            }
        }
    }
}
