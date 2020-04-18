use std::time::{Duration, Instant};

pub struct Timer {
    start: Option<Instant>
}

impl Timer {
    pub fn new() -> Timer {
        Timer {
            start: None
        }
    }

    pub fn start(&mut self) {
        self.start.replace(Instant::now());
    }

    pub fn time(&self) -> Duration {
        Instant::now() - self.start.unwrap()
    }

    pub fn stop(&mut self) {
        self.start.take();
    }
}