use rustereum::prelude::*;

#[contract]
struct C {
    count: u256,
}

#[contract]
impl C {
    pub fn f(&mut self) {
        let _y = 1;
    }
}

fn main() {}
