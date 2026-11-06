// Ports test/util/run_loop.test.cpp.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use mln_actor::run_loop::Priority;
use mln_actor::{RunLoop, Timer};
use mln_util::chrono::Duration;

// gtest: RunLoop.Stop
#[test]
fn stop() {
    let run_loop = RunLoop::new(); // RunLoop::Type::New
    let timer = Timer::new();

    let l = run_loop.clone();
    timer.start(Duration::zero(), Duration::zero(), move || l.stop());

    run_loop.run();
}

// gtest: RunLoop.MultipleStop
#[test]
fn multiple_stop() {
    let run_loop = RunLoop::new();
    let timer = Timer::new();

    let l = run_loop.clone();
    timer.start(Duration::zero(), Duration::zero(), move || {
        l.stop();
        l.stop();
        l.stop();
        l.stop();
    });

    run_loop.run();
}

// gtest: RunLoop.MultipleRun
#[test]
fn multiple_run() {
    let run_loop = RunLoop::new();
    let timer = Timer::new();

    let l = run_loop.clone();
    timer.start(Duration::zero(), Duration::zero(), move || l.stop());
    run_loop.run();

    let second_timeout = Rc::new(Cell::new(false));
    let l = run_loop.clone();
    let s = second_timeout.clone();
    timer.start(Duration::zero(), Duration::zero(), move || {
        s.set(true);
        l.stop();
    });
    run_loop.run();

    assert!(second_timeout.get());
}

// gtest: RunLoop.Priorities
#[test]
fn priorities() {
    let order = Arc::new(Mutex::new(Vec::<i32>::new()));

    let run_loop = RunLoop::new();
    let o = order.clone();
    run_loop.invoke(move || o.lock().unwrap().push(1));
    let o = order.clone();
    run_loop.invoke_with_priority(Priority::High, move || o.lock().unwrap().push(2));
    let o = order.clone();
    run_loop.invoke(move || o.lock().unwrap().push(3));
    let o = order.clone();
    run_loop.invoke_with_priority(Priority::High, move || o.lock().unwrap().push(4));
    let handle = run_loop.handle();
    run_loop.invoke_with_priority(Priority::Default, move || handle.stop());
    run_loop.run();

    assert_eq!(vec![2, 4, 1, 3], *order.lock().unwrap());
}

// gtest: RunLoop.PlatformIntegration
#[test]
#[ignore = "TODO(port): RunLoop::setPlatformCallback"]
fn platform_integration() {
    // test/util/run_loop.test.cpp:68: two threads invoke 100000 tasks each on
    // the main loop; the platform callback (on the invoking threads) wakes
    // the main thread, which calls runOnce until all 200000 ran; both counts
    // must be 200000.
}
