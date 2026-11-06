# Memorize Client

A Rust client for the Memorize caching server.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
memorize-client = { path = "../path/to/memorize-client" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Quick Start

```rust
use memorize_client::{MemorizeClient, MemorizeClientConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to server
    let client = MemorizeClient::connect(MemorizeClientConfig {
        address: "http://localhost:50051".to_string(),
        api_key: Some("your-api-key".to_string()),
    }).await?;

    // Set a value with 5 minute TTL
    client.set("key", "value", Some(300)).await?;

    // Get the value
    if let Some(value) = client.get("key").await? {
        println!("Got: {}", value);
    }

    // Delete
    client.delete("key").await?;

    Ok(())
}
```

## Configuration from Environment

```rust
use memorize_client::{MemorizeClient, MemorizeClientConfig};

// Uses MEMORIZE_SERVER_ADDRESS and MEMORIZE_API_KEY
let config = MemorizeClientConfig::from_env();
let client = MemorizeClient::connect(config).await?;
```

## Thread Safety

The client is `Clone` and thread-safe. Clone it freely to share across tasks:

```rust
let client = MemorizeClient::connect(config).await?;

// Clone for each task
let client1 = client.clone();
let client2 = client.clone();

let handle1 = tokio::spawn(async move {
    client1.set("key1", "value1", None).await
});

let handle2 = tokio::spawn(async move {
    client2.set("key2", "value2", None).await
});

handle1.await??;
handle2.await??;
```

## Error Handling

```rust
use memorize_client::{MemorizeClient, Error};

async fn cache_value(client: &MemorizeClient, key: &str, value: &str) {
    match client.set(key, value, None).await {
        Ok(()) => println!("Cached successfully"),
        Err(Error::StorageFull(msg)) => {
            // Server has reached its maximum storage limit
            // Consider evicting old entries or alerting ops
            eprintln!("Cache storage full: {}", msg);
        }
        Err(Error::Connection(status)) => {
            eprintln!("Connection error: {}", status);
        }
        Err(e) => {
            eprintln!("Cache error: {}", e);
        }
    }
}

// Check error type
if let Err(e) = client.set("key", "value", None).await {
    if e.is_storage_full() {
        // Handle storage full specifically
    }
}
```

### Storage Full Error

By default the server evicts entries to make room when it reaches its storage limit (`MEMORIZE_MAX_STORAGE_MB`), so `set()` only returns `Error::StorageFull` when the server runs with `MEMORIZE_EVICTION_POLICY=noeviction`, or when a single value is larger than the whole limit:

```rust
match client.set("key", large_value, None).await {
    Ok(()) => {}
    Err(Error::StorageFull(_)) => {
        // Option 1: Try to free space
        client.delete("old-key").await?;
        client.set("key", large_value, None).await?;
        
        // Option 2: Use a shorter TTL
        client.set("key", value, Some(60)).await?;
        
        // Option 3: Continue without caching
        log::warn!("Cache full, skipping cache write");
    }
    Err(e) => return Err(e.into()),
}
```

## API Reference

### `MemorizeClient::connect(config) -> Result<Self, Error>`

Connects to the Memorize server.

### `client.get(key) -> Result<Option<String>, Error>`

Gets a value by key as a string. Returns `None` if the key doesn't exist. Returns `Error::InvalidUtf8` if the value isn't valid UTF-8 (use `get_bytes` for binary values).

### `client.get_bytes(key) -> Result<Option<Vec<u8>>, Error>`

Gets a value by key as raw bytes. Returns `None` if the key doesn't exist.

### `client.set(key, value, ttl_secs) -> Result<(), Error>`

Sets a string value (stored as UTF-8) with an optional TTL in seconds. Returns `Error::StorageFull` if the value can't be stored (see above).

### `client.set_bytes(key, value, ttl_secs) -> Result<(), Error>`

Sets a raw byte value, for binary payloads such as protobuf, MessagePack or compressed data. Strings and bytes share the same storage, so a value set with `set` can be read with `get_bytes`, and UTF-8 bytes set with `set_bytes` can be read with `get`.

### `client.delete(key) -> Result<bool, Error>`

Deletes a key. Returns `true` if the key existed.

## License

See repository root for license information.
