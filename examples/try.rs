use std::{thread, time::Duration};

use professor::ProfilerGuardBuilder;


fn run() -> usize {
    let mut sum = 0;
    loop {
        thread::sleep(Duration::from_millis(1));
        sum += 1;

        if sum == 1_000_000_000 {
            return sum;
        }
    }
}

fn main() {
    pretty_env_logger::init();
    ProfilerGuardBuilder::default().start().unwrap();

    run();
}
