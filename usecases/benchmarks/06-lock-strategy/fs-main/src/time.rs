use core::cell::RefCell;

pub trait TimeProvider {
    fn now(&self) -> u64;
}

#[derive(Default)]
pub struct MonotonicCounter {
    counter: RefCell<u64>,
}

impl MonotonicCounter {
    pub fn new() -> Self {
        Self {
            counter: RefCell::new(0),
        }
    }
}

impl TimeProvider for MonotonicCounter {
    fn now(&self) -> u64 {
        let mut counter = self.counter.borrow_mut();
        let val = *counter;
        *counter = val + 1;
        val
    }
}
