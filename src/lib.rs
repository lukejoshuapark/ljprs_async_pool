//! # ljprs_async_pool
//! 
//! Provides an async-friendly pool data structure using tokio.

use std::cell::RefCell;
use std::error;
use std::future::Future;
use std::mem;
use std::ops::{Deref, Drop};
use tokio::sync::{Mutex, mpsc};

/// An `AsyncPool` data structure backed by tokio mpsc channels.
pub struct AsyncPool<T, F, R, E>
where
    R: Future<Output = Result<T, E>>,
    E: error::Error,
    F: Fn() -> R
{
    f: F,
    tx: mpsc::Sender<T>,
    rx: Mutex<RefCell<mpsc::Receiver<T>>>
}

impl<T, F, R, E> AsyncPool<T, F, R, E>
where
    R: Future<Output = Result<T, E>>,
    E: error::Error,
    F: Fn() -> R
{
    /// Creates a new, empty AsyncPool with the provided maximum size and
    /// initializer function.
    pub fn new(max_size: usize, f: F) -> AsyncPool<T, F, R, E> {
        let (tx, rx) = mpsc::channel::<T>(max_size);
        AsyncPool {
            f,
            tx,
            rx: Mutex::new(RefCell::new(rx))
        }
    }

    /// Attempts to retrieve a value out of the pool.  If the pool is empty,
    /// `get` calls the initializer function provided when the pool was created.
    /// Because this function can fail, `get` returns a `Result` with the same
    /// error type as the initializer function.
    pub async fn get<'a>(&'a self) -> Result<AsyncPoolGuard<'a, T, F, R, E>, E> {
        let mut guard = self.rx.lock().await;
        let rx = guard.get_mut();

        match rx.try_recv() {
            Ok(item) => Ok(AsyncPoolGuard::new(item, &self)),
            Err(_) => (self.f)().await.map(|item| AsyncPoolGuard::new(item, &self))
        }
    }

    fn put(&self, item: T) {
        let _ = self.tx.try_send(item);
    }
}

/// `AsyncPoolGuard` ensures values are returned to the pool when they go out of
/// scope.  You can dereference an `AsyncPoolGuard` to make calls on the value
/// contained within.
pub struct AsyncPoolGuard<'a, T, F, R, E>
where
    R: Future<Output = Result<T, E>>,
    E: error::Error,
    F: Fn() -> R
{
    item: AsyncPoolItemContainer<T>,
    invalidated: bool,
    pool: &'a AsyncPool<T, F, R, E>
}

impl<'a, T, F, R, E> AsyncPoolGuard<'a, T, F, R, E>
where
    R: Future<Output = Result<T, E>>,
    E: error::Error,
    F: Fn() -> R
{
    fn new(item: T, pool: &'a AsyncPool<T, F, R, E>) -> AsyncPoolGuard<'a, T, F, R, E> {
        AsyncPoolGuard {
            item: AsyncPoolItemContainer::new(item),
            invalidated: false,
            pool
        }
    }

    /// If the value attached to this `AsyncPoolGuard` malfunctions or otherwise
    /// becomes unfit to put back into the pool after use, call `invalidate` to
    /// ensure the value is dropped when it goes out of scope.
    pub fn invalidate(&mut self) {
        self.invalidated = true
    }
}

impl<'a, T, F, R, E> Drop for AsyncPoolGuard<'a, T, F, R, E>
where
    R: Future<Output = Result<T, E>>,
    E: error::Error,
    F: Fn() -> R
{
    fn drop(&mut self) {
        if !self.invalidated {
            self.pool.put(self.item.take())
        }
    }
}

impl<'a, T, F, R, E> Deref for AsyncPoolGuard<'a, T, F, R, E>
where
    R: Future<Output = Result<T, E>>,
    E: error::Error,
    F: Fn() -> R
{
    type Target = T;

    fn deref(&self) -> &T {
        self.item.as_ref()
    }
}

enum AsyncPoolItemContainer<T> {
    Empty,
    Item(T)
}

impl<T> AsyncPoolItemContainer<T> {
    fn new(item: T) -> AsyncPoolItemContainer<T> {
        AsyncPoolItemContainer::Item(item)
    }

    fn as_ref(&self) -> &T {
        match self {
            AsyncPoolItemContainer::Item(item) => &item,
            AsyncPoolItemContainer::Empty => panic!("cannot reference empty AsyncPoolItemContainer")
        }
    }

    fn take(&mut self) -> T {
        let replaced_self = mem::replace(self, AsyncPoolItemContainer::Empty);
        match replaced_self {
            AsyncPoolItemContainer::Item(item) => item,
            AsyncPoolItemContainer::Empty => panic!("cannot take empty AsyncPoolItemContainer")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[tokio::test]
    async fn test_gets_value_from_pool() {
        async fn initializer_fn() -> Result<i32, io::Error> {
            Ok(42)
        }

        let pool = AsyncPool::new(16, initializer_fn);
        let guard = pool.get().await;
        assert!(guard.is_ok());
        assert_eq!(*(guard.unwrap()), 42);
    }

    #[tokio::test]
    async fn test_puts_item_back_in_pool() {
        async fn initializer_fn() -> Result<i32, io::Error> {
            panic!("uh oh, this wasn't meant to happen")
        }

        let pool = AsyncPool::new(16, initializer_fn);
        {
            let _ = AsyncPoolGuard::new(42, &pool);
        }

        let guard = pool.get().await;
        assert!(guard.is_ok());
        assert_eq!(*(guard.unwrap()), 42);
    }

    #[tokio::test]
    async fn test_returns_error_when_initializer_fn_fails() {
        async fn initializer_fn() -> Result<i32, io::Error> {
            Err(io::Error::new(io::ErrorKind::Other, "uh oh"))
        }

        let pool = AsyncPool::new(16, initializer_fn);
        let guard = pool.get().await;
        assert!(guard.is_err());
    }

    #[tokio::test]
    async fn test_invalidates_guard() {
        async fn initializer_fn() -> Result<i32, io::Error> {
            Ok(42)
        }

        let pool = AsyncPool::new(16, initializer_fn);
        {
            let mut guard = AsyncPoolGuard::new(24, &pool);
            guard.invalidate();
        }

        let guard = pool.get().await;
        assert!(guard.is_ok());
        assert_eq!(*(guard.unwrap()), 42);
    }
}
