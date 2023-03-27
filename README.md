![](icon.png)

# ljprs_async_pool

Provides an async-friendly pool data structure using tokio.

## Usage Example

```rs
use std::io;
use ljprs_async_pool::AsyncPool;

async fn initializer_fn() -> Result<i32, io::Error> {
    Ok(42)
}

let pool = AsyncPool::new(16, initializer_fn);
let guard = pool.get().await;
assert!(guard.is_ok());
assert_eq!(*(guard.unwrap()), 42);
```
