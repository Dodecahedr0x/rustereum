use rustereum::prelude::*;

#[contract]
struct C {
    count: u256,
}

#[contract]
impl C {
    pub fn set(&mut self, x: u256) {
        self.count += 1;
    }
}

fn main() {}
