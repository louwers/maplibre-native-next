// Ports libstdc++ (GCC 16) bits/stl_heap.h (`__push_heap`, `__adjust_heap`,
// `__pop_heap`, `__make_heap`, `__sort_heap` and the public wrappers) and
// std::priority_queue from bits/stl_queue.h.
//
// libstdc++ computes with the signed `difference_type`; `isize` keeps its
// truncating division (`(0 - 1) / 2 == 0`).

/// `__push_heap(first, holeIndex, topIndex, value)` on `v[first..]`.
fn push_heap_impl<T: Copy, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mut hole_index: isize,
    top_index: isize,
    value: T,
    comp: &mut F,
) {
    let mut parent = (hole_index - 1) / 2;
    while hole_index > top_index && comp(&v[first + parent as usize], &value) {
        v[first + hole_index as usize] = v[first + parent as usize];
        hole_index = parent;
        parent = (hole_index - 1) / 2;
    }
    v[first + hole_index as usize] = value;
}

/// `__adjust_heap(first, holeIndex, len, value)` on `v[first..]`.
pub(crate) fn adjust_heap_range<T: Copy, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mut hole_index: isize,
    len: isize,
    value: T,
    comp: &mut F,
) {
    let top_index = hole_index;
    let mut second_child = hole_index;
    while second_child < (len - 1) / 2 {
        second_child = 2 * (second_child + 1);
        if comp(&v[first + second_child as usize], &v[first + (second_child - 1) as usize]) {
            second_child -= 1;
        }
        v[first + hole_index as usize] = v[first + second_child as usize];
        hole_index = second_child;
    }
    if (len & 1) == 0 && second_child == (len - 2) / 2 {
        second_child = 2 * (second_child + 1);
        v[first + hole_index as usize] = v[first + (second_child - 1) as usize];
        hole_index = second_child - 1;
    }
    push_heap_impl(v, first, hole_index, top_index, value, comp);
}

/// `__pop_heap(first, last, result)`: `result` may lie outside
/// `[first, last)` (as in `__heap_select`).
fn pop_heap_impl<T: Copy, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    result: usize,
    comp: &mut F,
) {
    let value = v[result];
    v[result] = v[first];
    adjust_heap_range(v, first, 0, (last - first) as isize, value, comp);
}

/// `__make_heap(first, last)`.
pub(crate) fn make_heap_range<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], first: usize, last: usize, comp: &mut F) {
    if last - first < 2 {
        return;
    }
    let len = (last - first) as isize;
    let mut parent = (len - 2) / 2;
    loop {
        let value = v[first + parent as usize];
        adjust_heap_range(v, first, parent, len, value, comp);
        if parent == 0 {
            return;
        }
        parent -= 1;
    }
}

/// `__sort_heap(first, last)`.
pub(crate) fn sort_heap_range<T: Copy, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mut last: usize,
    comp: &mut F,
) {
    while last - first > 1 {
        last -= 1;
        pop_heap_impl(v, first, last, last, comp);
    }
}

/// `std::make_heap(v.begin(), v.end(), comp)`.
pub fn make_heap<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut comp: F) {
    make_heap_range(v, 0, v.len(), &mut comp);
}

/// `std::push_heap(v.begin(), v.end(), comp)`: `v[..len - 1]` is a heap and
/// the last element is pushed into it.
pub fn push_heap<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut comp: F) {
    let len = v.len();
    if len == 0 {
        return;
    }
    let value = v[len - 1];
    push_heap_impl(v, 0, (len - 1) as isize, 0, value, &mut comp);
}

/// `std::pop_heap(v.begin(), v.end(), comp)`: moves the top to the end.
pub fn pop_heap<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut comp: F) {
    let len = v.len();
    if len > 1 {
        pop_heap_impl(v, 0, len - 1, len - 1, &mut comp);
    }
}

/// `std::sort_heap(v.begin(), v.end(), comp)`.
pub fn sort_heap<T: Copy, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut comp: F) {
    sort_heap_range(v, 0, v.len(), &mut comp);
}

/// `std::priority_queue<T, std::vector<T>, Compare>`: the top is the
/// greatest element under `comp`; among equal ones, the one libstdc++'s heap
/// puts there.
#[derive(Clone, Debug)]
pub struct PriorityQueue<T, F> {
    c: Vec<T>,
    comp: F,
}

impl<T: Copy, F: FnMut(&T, &T) -> bool> PriorityQueue<T, F> {
    /// `priority_queue(const Compare&)`.
    pub fn new(comp: F) -> Self {
        PriorityQueue { c: Vec::new(), comp }
    }

    pub fn is_empty(&self) -> bool {
        self.c.is_empty()
    }

    pub fn len(&self) -> usize {
        self.c.len()
    }

    /// `top()`; panics when empty (UB in C++).
    pub fn top(&self) -> &T {
        &self.c[0]
    }

    /// `push(x)`: `c.push_back(x); std::push_heap(c.begin(), c.end(), comp)`.
    pub fn push(&mut self, x: T) {
        self.c.push(x);
        push_heap(&mut self.c, &mut self.comp);
    }

    /// `pop()`: `std::pop_heap(c.begin(), c.end(), comp); c.pop_back()`.
    pub fn pop(&mut self) {
        pop_heap(&mut self.c, &mut self.comp);
        self.c.pop();
    }

    /// The underlying container (`c`), in heap order.
    pub fn as_slice(&self) -> &[T] {
        &self.c
    }
}
