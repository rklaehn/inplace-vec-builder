//! A data structure for in place modification of smallvecs.
#![deny(missing_docs)]
#![allow(dead_code)]
use core::fmt::Debug;
use smallvec::{Array, SmallVec};

/// builds a SmallVec out of itself
///
/// Takes over the source vector as storage and gives it back on drop.
/// This is not observable unless you prevent drop from running using [`std::mem::forget`].
/// If you prevent drop from running, the source vector will be empty and the contents
/// are leaked.
pub struct InPlaceSmallVecBuilder<'a, A: Array> {
    /// the underlying vector. While the builder is alive its `len` is kept at 0
    /// and it is treated as raw storage: the target lives in `[0..t1)` and the
    /// source in `[s0..s1)`, both beyond the (zero) length. `Drop` sets `len` to `t1`.
    v: &'a mut SmallVec<A>,
    /// the end of the target area
    t1: usize,
    /// the start of the source area
    s0: usize,
    /// the end of the source area (in spare capacity, beyond `v.len()`)
    s1: usize,
}

impl<'a, T: Debug, A: Array<Item = T>> Debug for InPlaceSmallVecBuilder<'a, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let InPlaceSmallVecBuilder { s0, t1, s1, v } = self;
        let cap = v.capacity();
        write!(
            f,
            "InPlaceSmallVecBuilder(0..{},{}..{},{})",
            t1, s0, s1, cap
        )
    }
}

/// initializes the source part of this flip buffer with the given vector.
/// The target part is initially empty.
impl<'a, A: Array> From<&'a mut SmallVec<A>> for InPlaceSmallVecBuilder<'a, A> {
    fn from(value: &'a mut SmallVec<A>) -> Self {
        let s1 = value.len();
        // Take over the vec as raw storage: set its len to 0. This does NOT drop
        // anything; the source bytes remain in spare capacity `[0..s1)`.
        unsafe {
            value.set_len(0);
        }
        InPlaceSmallVecBuilder {
            v: value,
            s0: 0,
            t1: 0,
            s1,
        }
    }
}

impl<'a, A: Array> InPlaceSmallVecBuilder<'a, A> {
    /// The current target part as a slice
    pub fn target_slice(&self) -> &[A::Item] {
        // `v.len()` is 0 while the builder is alive, so the target `[0..t1)` is in
        // spare capacity; slice it directly.
        unsafe { std::slice::from_raw_parts(self.v.as_ptr(), self.t1) }
    }

    /// The current source part as a slice
    pub fn source_slice(&self) -> &[A::Item] {
        // The source lives in spare capacity beyond `v.len()`, so we cannot use
        // ordinary slicing. The elements `[s0..s1)` are initialized.
        unsafe { std::slice::from_raw_parts(self.v.as_ptr().add(self.s0), self.s1 - self.s0) }
    }

    /// The current source part as a slice
    pub fn source_slice_mut(&mut self) -> &mut [A::Item] {
        // The source lives in spare capacity beyond `v.len()`, so we cannot use
        // ordinary slicing. The elements `[s0..s1)` are initialized.
        unsafe {
            std::slice::from_raw_parts_mut(self.v.as_mut_ptr().add(self.s0), self.s1 - self.s0)
        }
    }

    /// ensure that we have at least `capacity` space.
    #[inline]
    fn reserve(&mut self, capacity: usize) {
        // ensure we have space!
        if self.t1 + capacity > self.s0 {
            let v = &mut self.v;
            let sn = self.s1 - self.s0;
            // Momentarily extend `len` to cover the source so that a realloc
            // inside `SmallVec::reserve` preserves it (`reserve` only copies `[0..len)`).
            // This transient `len == s1` window is sound because no user code runs
            // in it, so `forget` cannot be slipped in to skip the fixup. If `reserve`
            // unwinds (capacity overflow) our `Drop` restores the length and drops
            // the source; if it aborts (OOM) the process dies. Either way the corrupt
            // `len` is never exposed.
            unsafe {
                v.set_len(self.s1);
            }
            // delegate to the underlying vec for the grow logic
            v.reserve(capacity);
            // move the source to the end of the vec
            let cap = v.capacity();
            unsafe {
                // just move source to the end without any concern about dropping
                copy(v.as_mut_ptr(), self.s0, cap - sn, sn);
                // restore len to 0; the target stays in spare capacity until Drop
                v.set_len(0);
            }
            // move the source cursors
            self.s0 = cap - sn;
            self.s1 = cap;
        }
    }

    /// Take at most `n` elements from `iter` to the target
    #[inline]
    pub fn extend_from_iter<I: Iterator<Item = A::Item>>(&mut self, mut iter: I, n: usize) {
        if n > 0 {
            self.reserve(n);
            for _ in 0..n {
                if let Some(value) = iter.next() {
                    self.push_unsafe(value)
                }
            }
        }
    }

    /// Push a single value to the target
    pub fn push(&mut self, value: A::Item) {
        // ensure we have space!
        self.reserve(1);
        self.push_unsafe(value);
    }

    fn push_unsafe(&mut self, value: A::Item) {
        unsafe { std::ptr::write(self.v.as_mut_ptr().add(self.t1), value) }
        self.t1 += 1;
    }

    /// Consume `n` elements from the source. If `take` is true they will be added to the target,
    /// else they will be dropped.
    #[inline]
    pub fn consume(&mut self, n: usize, take: bool) {
        let n = std::cmp::min(n, self.source_slice().len());
        let v = self.v.as_mut_ptr();
        if take {
            if self.t1 != self.s0 {
                unsafe {
                    copy(v, self.s0, self.t1, n);
                }
            }
            self.t1 += n;
            self.s0 += n;
        } else {
            for _ in 0..n {
                unsafe {
                    self.s0 += 1;
                    std::ptr::drop_in_place(v.add(self.s0 - 1));
                }
            }
        }
    }

    /// Skip up to `n` elements from source without adding them to the target.
    /// They will be immediately dropped!
    pub fn skip(&mut self, n: usize) {
        let n = std::cmp::min(n, self.source_slice().len());
        let v = self.v.as_mut_ptr();
        for _ in 0..n {
            unsafe {
                self.s0 += 1;
                std::ptr::drop_in_place(v.add(self.s0 - 1));
            }
        }
    }

    /// Take up to `n` elements from source to target.
    /// If n is larger than the size of the remaining source, this will only copy all remaining elements in source.
    pub fn take(&mut self, n: usize) {
        let n = std::cmp::min(n, self.source_slice().len());
        if self.t1 != self.s0 {
            unsafe {
                copy(self.v.as_mut_ptr(), self.s0, self.t1, n);
            }
        }
        self.t1 += n;
        self.s0 += n;
    }

    /// Takes the next element from the source, if it exists
    pub fn pop_front(&mut self) -> Option<A::Item> {
        if self.s0 < self.s1 {
            self.s0 += 1;
            Some(unsafe { std::ptr::read(self.v.as_ptr().add(self.s0 - 1)) })
        } else {
            None
        }
    }

    fn drop_source(&mut self) {
        // While the builder was alive `v.len()` was 0. First expose the finished
        // target prefix by setting the length to `t1` (done before the drop, so a
        // panicking source destructor still leaves a valid vec). Then drop the
        // source elements, which live in spare capacity `[s0..s1)`.
        let start = self.s0;
        let len = self.s1 - self.s0;
        self.s1 = self.s0;
        unsafe {
            self.v.set_len(self.t1);
            std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                self.v.as_mut_ptr().add(start),
                len,
            ));
        }
    }
}

#[inline]
unsafe fn copy<T>(v: *mut T, from: usize, to: usize, n: usize) {
    // if to < from {
    //     for i in 0..n {
    //         std::ptr::write(v.add(to + i), std::ptr::read(v.add(from + i)));
    //     }
    // } else {
    //     for i in (0..n).rev() {
    //         std::ptr::write(v.add(to + i), std::ptr::read(v.add(from + i)));
    //     }
    // }
    std::ptr::copy(v.add(from), v.add(to), n);
}

/// the purpose of drop is to clean up and make the SmallVec that we reference into a normal
/// SmallVec again.
impl<'a, A: Array> Drop for InPlaceSmallVecBuilder<'a, A> {
    fn drop(&mut self) {
        // drop the source part.
        self.drop_source();
    }
}

#[cfg(test)]
mod tests {
    extern crate testdrop;
    use super::*;
    use testdrop::{Item, TestDrop};

    type Array<'a> = [Item<'a>; 2];

    fn everything_dropped<'a, F>(td: &'a TestDrop, n: usize, f: F)
    where
        F: Fn(SmallVec<Array<'a>>, SmallVec<Array<'a>>),
    {
        let mut a: SmallVec<Array<'a>> = SmallVec::new();
        let mut b: SmallVec<Array<'a>> = SmallVec::new();
        let mut ids: Vec<usize> = Vec::new();
        for _ in 0..n {
            let (id, item) = td.new_item();
            a.push(item);
            ids.push(id);
        }
        for _ in 0..n {
            let (id, item) = td.new_item();
            b.push(item);
            ids.push(id);
        }
        f(a, b);
        for id in ids {
            td.assert_drop(id);
        }
    }

    #[test]
    fn drop_just_source() {
        everything_dropped(&TestDrop::new(), 10, |mut a, _| {
            let _: InPlaceSmallVecBuilder<Array> = (&mut a).into();
        })
    }

    #[test]
    fn target_push_gap() {
        everything_dropped(&TestDrop::new(), 10, |mut a, b| {
            let mut res: InPlaceSmallVecBuilder<Array> = (&mut a).into();
            for x in b.into_iter() {
                res.push(x);
            }
        })
    }

    #[test]
    fn source_move_some() {
        everything_dropped(&TestDrop::new(), 10, |mut a, _| {
            let mut res: InPlaceSmallVecBuilder<Array> = (&mut a).into();
            res.take(3);
        })
    }

    #[test]
    fn source_move_all() {
        everything_dropped(&TestDrop::new(), 10, |mut a, _| {
            let mut res: InPlaceSmallVecBuilder<Array> = (&mut a).into();
            res.take(10);
        })
    }

    #[test]
    fn source_drop_some() {
        everything_dropped(&TestDrop::new(), 10, |mut a, _| {
            let mut res: InPlaceSmallVecBuilder<Array> = (&mut a).into();
            res.skip(3);
        })
    }

    #[test]
    fn source_drop_all() {
        everything_dropped(&TestDrop::new(), 10, |mut a, _| {
            let mut res: InPlaceSmallVecBuilder<Array> = (&mut a).into();
            res.skip(10);
        })
    }

    #[test]
    fn source_pop_some() {
        everything_dropped(&TestDrop::new(), 10, |mut a, _| {
            let mut res: InPlaceSmallVecBuilder<Array> = (&mut a).into();
            res.pop_front();
            res.pop_front();
            res.pop_front();
        })
    }

    #[test]
    fn forget_does_not_corrupt_vec() {
        let td = TestDrop::new();
        let (a_id, a) = td.new_item();
        let (b_id, b) = td.new_item();
        let (x_id, x) = td.new_item();

        let mut vec: SmallVec<Array> = SmallVec::from_vec(vec![a, b]);
        {
            let mut builder: InPlaceSmallVecBuilder<Array> = (&mut vec).into();
            builder.push(x); // enters the intermediate post-reserve state
            std::mem::forget(builder);
        }
        // The builder was forgotten, so its `Drop` never ran: the vec is left
        // empty (a valid state), not with the corrupt `len == cap`. Every element
        // leaks in spare capacity, which is safe — `forget` opts into leaking.
        assert_eq!(vec.len(), 0);
        drop(vec); // empty vec: frees the buffer, runs no destructors

        td.assert_no_drop(x_id);
        td.assert_no_drop(a_id);
        td.assert_no_drop(b_id);
    }
}
