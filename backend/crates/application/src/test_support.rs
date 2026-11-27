//! Std-only helpers for driving async use cases in unit tests.
//!
//! The application crate must stay free of external dependencies (architecture check), so it
//! cannot use an async runtime in tests. Test fakes complete without suspending, which lets a
//! single poll with a no-op waker drive a use case to completion deterministically.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

pub(crate) trait BlockOn: Future + Sized {
    /// Polls the future once and returns its output.
    ///
    /// Panics if the future suspends: application test fakes must never wait on I/O.
    fn block_on(self) -> Self::Output {
        let mut future = pin!(self);
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("test fake suspended; fakes must complete synchronously"),
        }
    }
}

impl<F: Future> BlockOn for F {}
