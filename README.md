# Inplace-Vec-Builder

A small library to build a [Vec](https://doc.rust-lang.org/std/vec/struct.Vec.html) or [SmallVec](https://docs.rs/smallvec) out of itself without allocating.

This is useful when writing in place operations that do not allocate.

Imagine you have a vec that contains some numbers. You now want to apply some transformation on
these elements, like mapping, filtering, adding some elements, and then store the result in the same place.

The simplest way to do this would be something like this:

```rust
struct Foo {
    elements: Vec<u64>,
}

impl Foo {
    /// Keep elements > 5, double them, then append 123 - allocating a new vec.
    fn transform(&mut self) {
        let mut res: Vec<u64> = self
            .elements
            .iter()
            .filter(|x| **x > 5)
            .map(|x| *x * 2)
            .chain(std::iter::once(123))
            .collect();
        std::mem::swap(&mut self.elements, &mut res);
    }
}

let mut foo = Foo { elements: vec![1, 6, 7] };
foo.transform();
assert_eq!(foo.elements, vec![12, 14, 123]);
```

But this does allocate a new vector. Usually not a big deal, but if this is some very frequently used code, you want to avoid it.

Note that in many cases where you do filtering combined with a transformation, [retain](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.retain) can be used. If that is the case using retain is
of course preferable.

This crate provides a helper that allows doing something like the above without allocations. It is
fairly low level, since it is intended to be used from other libraries.

```rust
use inplace_vec_builder::InPlaceVecBuilder;

struct Foo {
    elements: Vec<u64>,
}

impl Foo {
    /// The same transformation as above, but reusing the existing allocation.
    fn transform(&mut self) {
        let mut t = InPlaceVecBuilder::from(&mut self.elements);
        while let Some(elem) = t.pop_front() {
            if elem > 5 {
                t.push(elem * 2);
            }
        }
        t.push(123);
    }
}

let mut foo = Foo { elements: vec![1, 6, 7] };
foo.transform();
assert_eq!(foo.elements, vec![12, 14, 123]);
```

# Features

- stdvec (default): std Vec support
- smallvec: SmallVec support
