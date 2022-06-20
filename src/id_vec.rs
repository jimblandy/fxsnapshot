//! The `IdVec` type, a vector indexed by a newtype around a `usize`.
#![allow(dead_code)]

use std::marker::PhantomData;
use std::ops::{Deref, DerefMut, Index, IndexMut};

/// An `IdVec<I, T>` is like a `Vec<T>` that uses `I` as its index type. `I` can
/// be any type that implements `IdVecIndex`, which converts back and forth to
/// `usize`; in practice, `IdVec` index types will be newtypes around `usize`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct IdVec<I, T>(Vec<T>, PhantomData<I>);

/// The trait for types that can index an `IdVec`.
///
/// This isn't just an extension of `From<usize>` and `Into<usize>` because
/// those traits are used everywhere, and whole point of newtyped indexes is to
/// avoid accidental conversions.
pub trait IdVecIndex {
    fn to_usize(self) -> usize;
    fn from_usize(size: usize) -> Self;
}

impl<I: IdVecIndex, T> IdVec<I, T>
    where I: IdVecIndex
{
    pub fn new() -> IdVec<I, T> {
        IdVec(Vec::new(), PhantomData)
    }

    pub fn with_capacity(capacity: usize) -> IdVec<I, T> {
        IdVec(Vec::with_capacity(capacity), PhantomData)
    }

    /// Push `value` onto the end of `self`, and assert that it appears at the
    /// given `index`.
    pub fn push_at(&mut self, index: I, value: T) {
        assert_eq!(self.0.len(), index.to_usize());
        self.0.push(value)
    }
}

impl<I, T> Default for IdVec<I, T> {
    fn default() -> IdVec<I, T> {
        IdVec(Vec::default(), PhantomData)
    }
}

impl<I, T> Deref for IdVec<I, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<I, T> DerefMut for IdVec<I, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

impl<I, T> Index<I> for IdVec<I, T>
    where I: IdVecIndex
{
    type Output = T;
    fn index(&self, index: I) -> &T {
        &self.0[index.to_usize()]
    }
}

impl<I, T> IndexMut<I> for IdVec<I, T>
    where I: IdVecIndex
{
    fn index_mut(&mut self, index: I) -> &mut T {
        &mut self.0[index.to_usize()]
    }
}

/// Implement the `IdVecIndex` trait for the type `t`, which must be a
/// single-field tuple struct around a `usize`.
macro_rules! impl_id_vec_index {
    ($t:ident) => {
        impl IdVecIndex for $t {
            fn to_usize(self) -> usize {
                self.0
            }

            fn from_usize(u: usize) -> Self {
                $t(u)
            }
        }
    }
}

impl<I, T> IntoIterator for IdVec<I, T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> std::vec::IntoIter<T> {
        self.0.into_iter()
    }
}

impl<I, A> std::iter::FromIterator<A> for IdVec<I, A> {
    fn from_iter<T: IntoIterator<Item = A>>(iter: T) -> Self {
         IdVec(Vec::from_iter(iter), PhantomData)
    }
}
