// Ports src/mln/util/thread_pool.hpp and src/mln/util/thread_pool.cpp:
// `ThreadedSchedulerBase` / `ThreadedScheduler` with one queue per tag, and
// `ThreadPool` (= `ParallelScheduler(3)`, 4 threads).
//
// TODO(port): `runOnRenderThread`/`runRenderJobs`, thread names/priorities
// from platform settings. (gtests-core added `SequencedScheduler` and the
// exception handler: a panicking task is caught and passed to the handler,
// as C++ catches the exception; without a handler the panic continues.)
//
// Port-only, wasm32-unknown-unknown (the browser build, tools/web-test),
// which has no threads: a scheduler starts no workers. Its tasks wait in the
// same tagged queues until the thread's RunLoop runs them
// (`run_background_tasks`, from `RunLoop::run_once`), a round at a time as a
// worker visits the queues, and `wait_for_empty` runs the tag's tasks itself
// instead of waiting for a worker. (A task that panics aborts the module
// there: the target has no unwinding, so the exception handler never runs.)

use crate::scheduler::{ExceptionHandler, Scheduler, Task};
use mln_util::identity::SimpleIdentity;
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

#[derive(Default)]
struct Queue {
    /// running tasks
    running_count: AtomicUsize,
    /// queue empty condition
    cv: Condvar,
    /// pending task queue (the C++ `lock` guards it)
    queue: Mutex<VecDeque<Task>>,
}

struct Shared {
    unique_id: SimpleIdentity,
    cv_available: Condvar,
    worker_mutex: Mutex<bool>, // terminated
    tagged_queue: Mutex<HashMap<SimpleIdentity, Arc<Queue>>>,
    task_count: AtomicUsize,
    /// `Scheduler::handler` (gtests-core).
    handler: Mutex<Option<Arc<ExceptionHandler>>>,
}

thread_local! {
    /// `owningThreadPool`.
    static OWNING_THREAD_POOL: Cell<usize> = const { Cell::new(0) };
}

/// `ThreadedScheduler(n)`.
pub struct ThreadedScheduler {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl ThreadedScheduler {
    pub fn new(n: usize) -> Self {
        let shared = Arc::new(Shared {
            unique_id: SimpleIdentity::new(),
            cv_available: Condvar::new(),
            worker_mutex: Mutex::new(false),
            tagged_queue: Mutex::new(HashMap::new()),
            task_count: AtomicUsize::new(0),
            handler: Mutex::new(None),
        });
        #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
        let threads = (0..n).map(|index| make_scheduler_thread(shared.clone(), index)).collect();
        #[cfg(all(target_family = "wasm", target_os = "unknown"))]
        let threads = {
            let _ = n;
            web::register(&shared);
            Vec::new()
        };
        ThreadedScheduler { shared, threads }
    }

    fn this_thread_is_owned(&self) -> bool {
        OWNING_THREAD_POOL.with(|p| p.get()) == Arc::as_ptr(&self.shared) as usize
    }
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
fn make_scheduler_thread(shared: Arc<Shared>, index: usize) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(format!("Worker {}", index + 1))
        .spawn(move || {
            OWNING_THREAD_POOL.with(|p| p.set(Arc::as_ptr(&shared) as usize));
            loop {
                {
                    let mut terminated = shared.worker_mutex.lock().unwrap();
                    if !*terminated && shared.task_count.load(Ordering::SeqCst) == 0 {
                        terminated = shared.cv_available.wait(terminated).unwrap();
                    }
                    if *terminated {
                        break;
                    }
                    // Let other threads run
                }

                // 1. Gather buckets for us to visit this iteration
                let pending: Vec<Arc<Queue>> = shared.tagged_queue.lock().unwrap().values().cloned().collect();

                // 2. Visit a task from each
                for q in pending {
                    let tasklet = {
                        let mut queue = q.queue.lock().unwrap();
                        let tasklet = queue.pop_front();
                        if tasklet.is_some() {
                            q.running_count.fetch_add(1, Ordering::SeqCst);
                        }
                        tasklet
                    };
                    let Some(tasklet) = tasklet else {
                        continue;
                    };

                    shared.task_count.fetch_sub(1, Ordering::SeqCst);

                    // gtests-core: C++ `try { ... } catch (...)`.
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(tasklet)) {
                        Ok(()) => {
                            // The function and its captures are gone before
                            // unblocking `waitForEmpty`.

                            if q.running_count.fetch_sub(1, Ordering::SeqCst) == 1 {
                                let queue = q.queue.lock().unwrap();
                                if queue.is_empty() {
                                    q.cv.notify_all();
                                }
                            }
                        }
                        Err(payload) => {
                            let queue = q.queue.lock().unwrap();
                            let handler = shared.handler.lock().unwrap().clone();
                            let payload = match &handler {
                                Some(handler) => {
                                    handler(payload);
                                    None
                                }
                                None => Some(payload),
                            };
                            if q.running_count.fetch_sub(1, Ordering::SeqCst) == 1 && queue.is_empty() {
                                q.cv.notify_all();
                            }
                            drop(queue);
                            if let Some(payload) = payload {
                                std::panic::resume_unwind(payload);
                            }
                        }
                    }
                }
            }
        })
        .expect("failed to spawn worker thread")
}

impl Scheduler for ThreadedScheduler {
    fn schedule(&self, task: Task) {
        self.schedule_tagged(self.shared.unique_id, task);
    }

    fn schedule_tagged(&self, tag: SimpleIdentity, task: Task) {
        let q = {
            let mut tagged_queue = self.shared.tagged_queue.lock().unwrap();
            // find or insert
            tagged_queue.entry(tag).or_default().clone()
        };

        {
            let mut queue = q.queue.lock().unwrap();
            queue.push_back(task);
            self.shared.task_count.fetch_add(1, Ordering::SeqCst);
        }

        // Take the worker lock before notifying to prevent threads from
        // waiting while we try to wake them
        let _worker_lock = self.shared.worker_mutex.lock().unwrap();
        self.shared.cv_available.notify_one();
    }

    fn wait_for_empty(&self) {
        self.wait_for_empty_tagged(SimpleIdentity::EMPTY);
    }

    fn set_exception_handler(&self, handler: ExceptionHandler) {
        *self.shared.handler.lock().unwrap() = Some(Arc::new(handler));
    }

    fn wait_for_empty_tagged(&self, tag: SimpleIdentity) {
        // Must not be called from a thread in our pool, or we would deadlock
        assert!(!self.this_thread_is_owned());
        if self.this_thread_is_owned() {
            return;
        }
        let tag_to_find = if tag.is_empty() { self.shared.unique_id } else { tag };
        let q = {
            let tagged_queue = self.shared.tagged_queue.lock().unwrap();
            match tagged_queue.get(&tag_to_find) {
                Some(q) => q.clone(),
                None => return,
            }
        };

        // The browser build has no workers to wait for: run the tag's tasks
        // here.
        #[cfg(all(target_family = "wasm", target_os = "unknown"))]
        web::drain(&self.shared, &q);

        let mut queue = q.queue.lock().unwrap();
        while queue.len() + q.running_count.load(Ordering::SeqCst) != 0 {
            queue = q.cv.wait(queue).unwrap();
        }
        drop(queue);

        // After waiting for the queue to empty, go ahead and erase it from the map.
        self.shared.tagged_queue.lock().unwrap().remove(&tag_to_find);
    }
}

impl Drop for ThreadedScheduler {
    fn drop(&mut self) {
        assert!(!self.this_thread_is_owned());
        // terminate(): wake up all threads so that they shut down
        *self.shared.worker_mutex.lock().unwrap() = true;
        self.shared.cv_available.notify_all();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// `ThreadPool`: `ParallelScheduler(3)`, i.e. four worker threads.
pub struct ThreadPool {
    scheduler: ThreadedScheduler,
}

impl Default for ThreadPool {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadPool {
    pub fn new() -> Self {
        ThreadPool {
            scheduler: ThreadedScheduler::new(1 + 3),
        }
    }
}

impl Scheduler for ThreadPool {
    fn schedule(&self, task: Task) {
        self.scheduler.schedule(task);
    }
    fn schedule_tagged(&self, tag: SimpleIdentity, task: Task) {
        self.scheduler.schedule_tagged(tag, task);
    }
    fn wait_for_empty(&self) {
        self.scheduler.wait_for_empty();
    }
    fn wait_for_empty_tagged(&self, tag: SimpleIdentity) {
        self.scheduler.wait_for_empty_tagged(tag);
    }
    fn set_exception_handler(&self, handler: ExceptionHandler) {
        self.scheduler.set_exception_handler(handler);
    }
}

/// `SequencedScheduler`: `ThreadedScheduler(1)` (gtests-core).
pub struct SequencedScheduler {
    scheduler: ThreadedScheduler,
}

impl Default for SequencedScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl SequencedScheduler {
    pub fn new() -> Self {
        SequencedScheduler {
            scheduler: ThreadedScheduler::new(1),
        }
    }
}

impl Scheduler for SequencedScheduler {
    fn schedule(&self, task: Task) {
        self.scheduler.schedule(task);
    }
    fn schedule_tagged(&self, tag: SimpleIdentity, task: Task) {
        self.scheduler.schedule_tagged(tag, task);
    }
    fn wait_for_empty(&self) {
        self.scheduler.wait_for_empty();
    }
    fn wait_for_empty_tagged(&self, tag: SimpleIdentity) {
        self.scheduler.wait_for_empty_tagged(tag);
    }
    fn set_exception_handler(&self, handler: ExceptionHandler) {
        self.scheduler.set_exception_handler(handler);
    }
}

/// The browser build (see the top of the file): runs the tasks queued in
/// every thread pool, a round at a time (one task from each tagged queue, as
/// a worker's iteration takes), until none are left.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn run_background_tasks() {
    web::run_background_tasks();
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
mod web {
    use super::{OWNING_THREAD_POOL, Queue, Shared};
    use std::cell::RefCell;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Weak};

    thread_local! {
        /// Every scheduler's queues (all on the one thread).
        static SCHEDULERS: RefCell<Vec<Weak<Shared>>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn register(shared: &Arc<Shared>) {
        SCHEDULERS.with(|schedulers| {
            let mut schedulers = schedulers.borrow_mut();
            schedulers.retain(|s| s.strong_count() > 0);
            schedulers.push(Arc::downgrade(shared));
        });
    }

    /// Runs `task` as a worker of `shared`'s pool (`owningThreadPool`).
    fn run_as_worker(shared: &Shared, q: &Queue, task: super::Task) {
        q.running_count.fetch_add(1, Ordering::SeqCst);
        let previous = OWNING_THREAD_POOL.with(|p| p.replace(shared as *const Shared as usize));
        task();
        OWNING_THREAD_POOL.with(|p| p.set(previous));
        q.running_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// One iteration of a worker: a task from each tagged queue. Returns
    /// whether it ran any.
    fn visit_queues(shared: &Shared) -> bool {
        let mut ran = false;
        let pending: Vec<Arc<Queue>> = shared.tagged_queue.lock().unwrap().values().cloned().collect();
        for q in pending {
            let Some(task) = q.queue.lock().unwrap().pop_front() else {
                continue;
            };
            shared.task_count.fetch_sub(1, Ordering::SeqCst);
            run_as_worker(shared, &q, task);
            ran = true;
        }
        ran
    }

    pub(super) fn run_background_tasks() {
        loop {
            let live: Vec<Arc<Shared>> =
                SCHEDULERS.with(|schedulers| schedulers.borrow().iter().filter_map(Weak::upgrade).collect());
            let mut ran = false;
            for shared in live {
                if shared.task_count.load(Ordering::SeqCst) != 0 {
                    ran |= visit_queues(&shared);
                }
            }
            if !ran {
                return;
            }
        }
    }

    /// `waitForEmpty(tag)`: runs `q`'s tasks until it is empty.
    pub(super) fn drain(shared: &Shared, q: &Queue) {
        loop {
            let Some(task) = q.queue.lock().unwrap().pop_front() else {
                return;
            };
            shared.task_count.fetch_sub(1, Ordering::SeqCst);
            run_as_worker(shared, q, task);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_loop::RunLoop;
    use crate::scheduler::schedule_and_reply_value;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn runs_tagged_tasks_and_waits() {
        let pool = ThreadPool::new();
        let tag = SimpleIdentity::new();
        let count = Arc::new(AtomicUsize::new(0));
        for _ in 0..50 {
            let c = count.clone();
            pool.schedule_tagged(
                tag,
                Box::new(move || {
                    c.fetch_add(1, Ordering::SeqCst);
                }),
            );
        }
        pool.wait_for_empty_tagged(tag);
        assert_eq!(count.load(Ordering::SeqCst), 50);
    }

    #[test]
    fn reply_runs_on_the_calling_loop() {
        let run_loop = RunLoop::new();
        let pool = ThreadPool::new();
        let result = Rc::new(RefCell::new(None));
        let r = result.clone();
        let main = std::thread::current().id();
        schedule_and_reply_value(
            &pool,
            SimpleIdentity::new(),
            move || {
                assert_ne!(std::thread::current().id(), main);
                6 * 7
            },
            move |value: i32| {
                assert_eq!(std::thread::current().id(), main);
                *r.borrow_mut() = Some(value);
            },
        );
        while result.borrow().is_none() {
            run_loop.run_once();
            std::thread::yield_now();
        }
        assert_eq!(*result.borrow(), Some(42));
    }
}
