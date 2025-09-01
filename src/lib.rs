use std::{
    any::Any,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread,
};

type Task = Box<dyn FnOnce() + Send>;
type BoxedAnySend = Box<dyn Any + Send>;

/// A thread pool for executing tasks concurrently using a fixed number of worker threads.
///
/// # Examples
///
/// ```
/// use thread_pool::ThreadPool;
/// let pool = ThreadPool::new(4);
/// let handle = pool.spawn(|| 42);
/// assert_eq!(handle.join(), 42);
/// ```
pub struct ThreadPool {
    task_sender: Sender<Task>,
}

impl ThreadPool {
    /// Creates a new thread pool with the specified number of threads.
    ///
    /// # Arguments
    ///
    /// * `num_threads` - The number of worker threads to spawn in the pool.
    ///
    /// # Examples
    ///
    /// ```
    /// let pool = thread_pool::ThreadPool::new(4);
    /// ```
    pub fn new(num_threads: usize) -> Self {
        // TODO - switch to mpmc when it's out from nightly channel so that mutex can be removed
        let (task_sender, task_receiver) = channel::<Task>();
        let task_receiver = Arc::new(Mutex::new(task_receiver));
        for _ in 0..num_threads {
            let task_receiver = task_receiver.clone();
            thread::spawn(move || {
                loop {
                    match {
                        let guard = task_receiver.lock().expect("Failed to lock receiver mutex");
                        guard.recv()
                    } {
                        Ok(task) => task(),
                        Err(_) => continue, // No task yet, keep waiting
                    }
                }
            });
        }

        Self { task_sender }
    }

    /// Spawns a task into the thread pool.
    ///
    /// Returns a [`JoinHandle`] that can be used to wait for the task to complete and get its result.
    /// If the task panics, the panic will be propagated when joining on the handle.
    ///
    /// # Arguments
    ///
    /// * `f` - The closure to execute in the thread pool. Must be `Send` and `'static`.
    ///
    /// # Examples
    ///
    /// ```
    /// let pool = thread_pool::ThreadPool::new(2);
    /// let handle = pool.spawn(|| 1 + 2);
    /// assert_eq!(handle.join(), 3);
    /// ```
    pub fn spawn<F, T>(&self, f: F) -> JoinHandle<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let (result_sender, result_receiver) = channel::<Result<T, BoxedAnySend>>();
        self.task_sender
            .send(Box::new(move || {
                let result = catch_unwind(AssertUnwindSafe(f));
                result_sender.send(result).unwrap_or_default() // The handle might be dropped before the task completes
            }))
            .expect("Failed to send task to thread pool");
        JoinHandle { result_receiver }
    }

    /// Runs a set of tasks in a scope, allowing them to borrow non-`'static` data.
    ///
    /// All tasks spawned within the scope are guaranteed to complete before this function returns.
    /// This enables safe borrowing of stack data in concurrent tasks.
    ///
    /// # Arguments
    ///
    /// * `f` - A closure that receives a [`Scope`] and spawns tasks within it.
    ///
    /// # Examples
    ///
    /// ```
    /// let pool = thread_pool::ThreadPool::new(2);
    /// let mut value = 0;
    /// pool.scope(|s| {
    ///     s.spawn(|| value += 1);
    /// });
    /// // value is guaranteed to be updated here
    /// ```
    pub fn scope<'pool, 'env, F, T>(&'pool self, f: F) -> T
    where
        F: for<'scope> FnOnce(&'scope Scope<'pool, 'scope, 'env>) -> T,
    {
        let scope = Scope {
            data: Arc::new(ScopeData {
                num_running_tasks: AtomicUsize::new(0),
            }),
            pool: self,
            _scope: PhantomData,
            _env: PhantomData,
        };

        let result = catch_unwind(AssertUnwindSafe(|| f(&scope)));

        while scope.data.num_running_tasks.load(Ordering::Acquire) > 0 {
            thread::yield_now();
        }

        match result {
            Ok(result) => result,
            Err(e) => resume_unwind(e),
        }
    }
}

/// A handle that can be used to wait for a spawned task to complete and retrieve its result.
///
/// If the task panics, the panic will be propagated when calling [`join`].
pub struct JoinHandle<T> {
    result_receiver: Receiver<Result<T, BoxedAnySend>>,
}

impl<T> JoinHandle<T> {
    /// Waits for the task to complete and returns its result.
    ///
    /// If the task panicked, this method will propagate the panic.
    ///
    /// # Examples
    ///
    /// ```
    /// let pool = thread_pool::ThreadPool::new(2);
    /// let handle = pool.spawn(|| 5);
    /// assert_eq!(handle.join(), 5);
    /// ```
    pub fn join(&self) -> T {
        match self
            .result_receiver
            .recv()
            .expect("Failed to receive result from thread")
        {
            Ok(r) => r,
            Err(e) => resume_unwind(e),
        }
    }
}

/// A scope for spawning tasks that can borrow non-`'static` data.
///
/// All tasks spawned in a scope are guaranteed to finish before the scope exits.
pub struct Scope<'pool, 'scope, 'env: 'scope> {
    data: Arc<ScopeData>,
    pool: &'pool ThreadPool,
    _scope: PhantomData<&'scope ()>,
    _env: PhantomData<&'env ()>,
}

impl<'pool, 'scope, 'env> Scope<'pool, 'scope, 'env> {
    /// Spawns a task within the scope.
    ///
    /// The task can borrow data from the stack, as long as it does not outlive the scope.
    /// Returns a [`JoinHandle`] to wait for the task and retrieve its result.
    ///
    /// # Arguments
    ///
    /// * `f` - The closure to execute. Can borrow from the scope.
    ///
    /// # Examples
    ///
    /// ```
    /// let pool = thread_pool::ThreadPool::new(2);
    /// let mut value = 0;
    /// pool.scope(|s| {
    ///     s.spawn(|| value += 1);
    /// });
    /// ```
    pub fn spawn<F, T>(&'scope self, f: F) -> JoinHandle<T>
    where
        F: FnOnce() -> T + Send + 'scope,
        T: Send + 'static,
    {
        let scope_data = self.data.clone();
        scope_data.num_running_tasks.fetch_add(1, Ordering::SeqCst);
        let f = unsafe {
            std::mem::transmute::<
                Box<dyn FnOnce() -> T + Send + 'scope>,
                Box<dyn FnOnce() -> T + Send + 'static>,
            >(Box::new(f))
        };

        self.pool.spawn(move || {
            let result = f();
            scope_data.num_running_tasks.fetch_sub(1, Ordering::SeqCst);
            result
        })
    }
}

struct ScopeData {
    num_running_tasks: AtomicUsize,
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn execute_task_in_pool_works() {
        let thread_pool = ThreadPool::new(2);
        let counter = Arc::new(Mutex::new(1 as usize));

        let result = thread_pool
            .spawn(move || {
                let guard = counter.lock().expect("Failed to lock mutex");
                *guard + 2
            })
            .join();

        assert_eq!(result, 3);
    }

    #[test]
    #[should_panic]
    fn panic_propagation_works() {
        let thread_pool = ThreadPool::new(2);
        thread_pool.spawn(|| panic!("panic on purpose")).join();
    }

    #[test]
    fn scoped_spawn_works() {
        let thread_pool = ThreadPool::new(1);
        let counter = AtomicUsize::new(0);
        thread_pool.scope(|s| {
            s.spawn(|| counter.fetch_add(1, Ordering::SeqCst));
            s.spawn(|| counter.fetch_add(2, Ordering::SeqCst));
        });
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn stress_test() {
        let thread_pool = ThreadPool::new(4);
        let counter = Arc::new(Mutex::new(0 as usize));
        let n_tasks = 10000;

        let mut handles = Vec::with_capacity(n_tasks);
        for _ in 0..n_tasks {
            let counter = counter.clone();
            handles.push(thread_pool.spawn(move || {
                let mut guard = counter.lock().expect("Failed to lock mutex");
                *guard += 1;
            }));
        }
        for handle in handles {
            handle.join();
        }

        assert_eq!(*counter.lock().unwrap(), n_tasks);
    }
}
