use rustereum::prelude::*;

#[contract]
struct C {
    count: u256,
}

#[contract]
impl C {
    pub fn f(&self) -> bool {
        true
    }
}

fn main() {}
