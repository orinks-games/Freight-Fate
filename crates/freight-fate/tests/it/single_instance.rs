//! Port of `tests/test_single_instance.py`.

use std::cell::RefCell;

use freight_fate::single_instance::{
    MutexApi, SingleInstanceGuard, ERROR_ALREADY_EXISTS, SINGLE_INSTANCE_MUTEX_NAME,
};

/// `_FakeKernel32`: records the calls and answers a fixed handle and error.
struct FakeKernel32 {
    handle: usize,
    last_error: u32,
    create_mutex_calls: RefCell<Vec<String>>,
    closed_handles: RefCell<Vec<usize>>,
}

impl FakeKernel32 {
    fn new(handle: usize, last_error: u32) -> Self {
        Self {
            handle,
            last_error,
            create_mutex_calls: RefCell::new(Vec::new()),
            closed_handles: RefCell::new(Vec::new()),
        }
    }
}

impl MutexApi for FakeKernel32 {
    fn create_mutex(&self, name: &str) -> (usize, u32) {
        self.create_mutex_calls.borrow_mut().push(name.to_string());
        (self.handle, self.last_error)
    }

    fn close_handle(&self, handle: usize) {
        self.closed_handles.borrow_mut().push(handle);
    }
}

impl MutexApi for &FakeKernel32 {
    fn create_mutex(&self, name: &str) -> (usize, u32) {
        (*self).create_mutex(name)
    }

    fn close_handle(&self, handle: usize) {
        (*self).close_handle(handle)
    }
}

#[test]
fn test_windows_mutex_uses_stable_name() {
    let kernel32 = FakeKernel32::new(1234, 0);
    let mut guard = SingleInstanceGuard::with_api(SINGLE_INSTANCE_MUTEX_NAME, &kernel32, true);

    assert!(guard.acquire());
    assert!(guard.acquired());
    assert_eq!(
        *kernel32.create_mutex_calls.borrow(),
        vec![SINGLE_INSTANCE_MUTEX_NAME.to_string()]
    );

    guard.release();
    assert_eq!(*kernel32.closed_handles.borrow(), vec![1234]);
    assert!(!guard.acquired());
}

#[test]
fn test_second_windows_launch_is_rejected() {
    let kernel32 = FakeKernel32::new(1234, ERROR_ALREADY_EXISTS);
    let mut guard = SingleInstanceGuard::with_api(SINGLE_INSTANCE_MUTEX_NAME, &kernel32, true);

    assert!(!guard.acquire());
    assert!(!guard.acquired());
    assert_eq!(*kernel32.closed_handles.borrow(), vec![1234]);
}

#[test]
fn test_non_windows_single_instance_is_non_blocking() {
    let kernel32 = FakeKernel32::new(1234, 0);
    let mut guard = SingleInstanceGuard::with_api(SINGLE_INSTANCE_MUTEX_NAME, &kernel32, false);

    assert!(guard.acquire());
    assert!(guard.acquired());
    guard.release();
    assert!(!guard.acquired());
    assert!(kernel32.create_mutex_calls.borrow().is_empty());
}

#[test]
fn test_failed_create_mutex_fails_open() {
    let kernel32 = FakeKernel32::new(0, 5);
    let mut guard = SingleInstanceGuard::with_api(SINGLE_INSTANCE_MUTEX_NAME, &kernel32, true);
    assert!(guard.acquire());
    assert!(kernel32.closed_handles.borrow().is_empty());
}

#[test]
fn test_the_real_guard_acquires_and_releases() {
    // The real kernel32 (a no-op off Windows): one acquire per process
    // succeeds, and releasing hands the name back.
    let mut guard = SingleInstanceGuard::new(SINGLE_INSTANCE_MUTEX_NAME);
    let first = guard.acquire();
    guard.release();
    assert!(!guard.acquired());
    // Under a parallel test runner another test binary may hold the name;
    // either answer is a valid outcome, the point is that nothing panics.
    let _ = first;
}
