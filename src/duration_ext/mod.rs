use std::time::Duration;

pub trait DurationExt {
    fn time_fmt(&self) -> String;
}

impl DurationExt for Duration {
    fn time_fmt(&self) -> String {
        let minutes = self.as_secs() / 60;
        let seconds = self.as_secs() % 60;
        format!("{}:{:02}", minutes, seconds)
    }
}