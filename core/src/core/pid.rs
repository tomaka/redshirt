
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Pid(u64);

pub struct PidPool {
    next: u64,
}

impl PidPool {
    pub fn new() -> Self {
        PidPool {
            next: 0,
        }
    }

    pub fn assign(&mut self) -> Pid {
        let id = self.next;
        self.next += 1;
        Pid(id)
    }
}
