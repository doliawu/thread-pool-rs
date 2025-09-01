# Thread Pool

A simple thread pool implementation in Rust that aims to match what `std::thread::spawn` and `std::thread::scope` provide while remove the overhead of thread spawning and destroying.

## Features
- Fixed-size thread pool
- Task queue
- Scoped task spawning (scope)
- Join handles for task results (handle)

## Scope Feature

The `scope` feature allows you to spawn tasks that can borrow data from the stack, ensuring all tasks complete before the scope exits. This is useful for safely sharing references with tasks without requiring `'static` lifetimes.

Example:

```rust
thread_pool.scope(|s| {
    s.spawn(|| {
        // Task code that can borrow from the stack
    });
});
// All tasks spawned in the scope are guaranteed to finish before this line
```

## Handle Feature

When you spawn a task using `spawn`, you receive a `JoinHandle`. This handle allows you to wait for the task to complete and retrieve its result. If the task panics, the panic will be propagated when calling `join()`.

Example:

```rust
let handle = thread_pool.spawn(|| 42);
let result = handle.join();
assert_eq!(result, 42);
```

## Usage

Import and use the thread pool in your project:

```rust
use thread_pool::ThreadPool;

fn main() {
    let pool = ThreadPool::new(4);
    for i in 0..8 {
        pool.execute(move || {
            println!("Task {} is running", i);
        });
    }
}
```

## Running Tests

To run the tests:

```sh
cargo test
```

## Running bench mark
```sh
cargo bench

Running benches/task_execution.rs (target/release/deps/task_execution-dc7ad6e0b951c876)
Gnuplot not found, using plotters backend
thread_pool             time:   [4.5000 µs 4.5200 µs 4.5457 µs]
                        change: [−74.362% −74.022% −73.705%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 10 outliers among 100 measurements (10.00%)
  2 (2.00%) high mild
  8 (8.00%) high severe
```

## License

This project is licensed under the MIT License.
