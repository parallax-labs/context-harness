# Rust Best Practices at Acme

These are our team conventions for writing production Rust code.

---

## Project Structure

```
service-name/
├── Cargo.toml
├── src/
│   ├── main.rs           # Binary entry point
│   ├── lib.rs            # Library root (for testing)
│   ├── config.rs         # Configuration parsing
│   ├── api/              # HTTP handlers
│   │   ├── mod.rs
│   │   ├── routes.rs
│   │   └── handlers.rs
│   ├── domain/           # Business logic (no I/O)
│   │   ├── mod.rs
│   │   └── models.rs
│   ├── infra/            # External integrations
│   │   ├── mod.rs
│   │   ├── database.rs
│   │   └── kafka.rs
│   └── telemetry.rs      # Observability setup
├── tests/
│   └── integration/
│       └── api_test.rs
└── migrations/
    └── 001_initial.sql
```

---

## Error Handling

### Use `thiserror` for Library Errors

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum OrderError {
    #[error("Order not found: {0}")]
    NotFound(String),
    
    #[error("Payment declined: {reason}")]
    PaymentDeclined { reason: String },
    
    #[error("Insufficient inventory for product {product_id}")]
    InsufficientInventory { product_id: String },
    
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}
```

### Use `anyhow` for Application Code

```rust
use anyhow::{Context, Result};

async fn process_order(order_id: &str) -> Result<()> {
    let order = db::get_order(order_id)
        .await
        .context("Failed to fetch order from database")?;
    
    let payment = payment::charge(&order)
        .await
        .context("Payment processing failed")?;
    
    Ok(())
}
```

### Never Panic in Production

```rust
// ❌ Bad: panics on None
let user = users.get(id).unwrap();

// ✅ Good: returns error
let user = users.get(id)
    .ok_or_else(|| OrderError::NotFound(id.to_string()))?;

// ❌ Bad: panics on parse failure  
let port: u16 = env::var("PORT").unwrap().parse().unwrap();

// ✅ Good: provides context
let port: u16 = env::var("PORT")
    .context("PORT environment variable not set")?
    .parse()
    .context("PORT must be a valid u16")?;
```

---

## Async Patterns

### Use Tokio as the Runtime

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(config.db.max_connections)
        .connect(&config.db.url)
        .await?;
    
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/orders", post(create_order))
        .with_state(AppState { pool });
    
    let listener = TcpListener::bind(&config.bind).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
```

### Structured Concurrency

```rust
// ✅ Good: bounded concurrency with buffer_unordered
use futures::stream::{self, StreamExt};

let results: Vec<Result<Response>> = stream::iter(urls)
    .map(|url| async move { reqwest::get(&url).await })
    .buffer_unordered(10)  // Max 10 concurrent requests
    .collect()
    .await;

// ❌ Bad: unbounded spawn
for url in urls {
    tokio::spawn(async move { reqwest::get(&url).await });
}
```

### Graceful Shutdown

```rust
let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

tokio::spawn(async move {
    tokio::signal::ctrl_c().await.unwrap();
    shutdown_tx.send(()).unwrap();
});

axum::serve(listener, app)
    .with_graceful_shutdown(async { shutdown_rx.await.ok(); })
    .await?;
```

---

## Testing

### Unit Tests (Pure Logic)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_shipping() {
        let order = Order {
            items: vec![item(10_00, 2), item(25_00, 1)],
            destination: Address::domestic(),
        };
        
        assert_eq!(calculate_shipping(&order), 5_99);
    }
    
    #[test]
    fn test_free_shipping_over_threshold() {
        let order = Order {
            items: vec![item(50_00, 1)],
            destination: Address::domestic(),
        };
        
        assert_eq!(calculate_shipping(&order), 0);
    }
}
```

### Integration Tests (With Database)

```rust
#[sqlx::test(migrations = "migrations")]
async fn test_create_and_fetch_order(pool: PgPool) {
    let order = CreateOrder {
        user_id: "usr_123".into(),
        items: vec![OrderItem { product_id: "prod_456".into(), quantity: 2 }],
    };
    
    let created = OrderRepo::create(&pool, &order).await.unwrap();
    assert!(created.id.starts_with("ord_"));
    
    let fetched = OrderRepo::get(&pool, &created.id).await.unwrap();
    assert_eq!(fetched.user_id, "usr_123");
    assert_eq!(fetched.items.len(), 1);
}
```

---

## Performance

### Avoid Unnecessary Allocations

```rust
// ❌ Bad: allocates a new String
fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

// ✅ Good: accepts any string-like type
fn process(input: impl AsRef<str>) {
    let s = input.as_ref();
    // work with &str
}

// ✅ Good: use Cow for conditional ownership
use std::borrow::Cow;

fn normalize(input: &str) -> Cow<'_, str> {
    if input.contains(' ') {
        Cow::Owned(input.replace(' ', "_"))
    } else {
        Cow::Borrowed(input)
    }
}
```

### Use `#[inline]` Judiciously

Only inline small, hot functions. Let the compiler decide for everything else.

### Profile Before Optimizing

```bash
# CPU profiling
cargo flamegraph --bin order-service

# Memory profiling  
DHAT_OUT_FILE=dhat.json cargo run --features dhat-heap
```

---

## Dependencies

### Approved Crates

| Category | Crate | Version Policy |
|----------|-------|----------------|
| HTTP server | axum | Latest stable |
| HTTP client | reqwest | Latest stable |
| Database | sqlx | Latest stable |
| Serialization | serde + serde_json | Latest stable |
| Async runtime | tokio | Latest stable |
| Error handling | anyhow + thiserror | Latest stable |
| Logging | tracing | Latest stable |
| CLI | clap | Latest stable |
| Time | chrono | Latest stable |
| UUID | uuid | Latest stable |

### Adding New Dependencies

1. Check if an existing approved crate covers the use case
2. Evaluate: maintenance activity, security history, compile time impact
3. Get approval from team lead for new dependencies
4. Run `cargo audit` after adding

