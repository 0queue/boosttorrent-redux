use std::time::Duration;
use std::time::Instant;

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

    pub fn time(&self) -> Option<Duration> {
        self.start.map(|i| Instant::now() - i)
    }

    pub fn stop(&mut self) {
        self.start.take();
    }
}