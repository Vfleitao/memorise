//! Memorize Integration Tests
//!
//! Tests the Memorize cache server using the memorize-client library.
//! This proves that the Rust client binding works correctly.

use anyhow::Result;
use futures::future::join_all;
use memorize_client::{MemorizeClient, MemorizeClientOptions};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "memorize_integration_tests=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let options = MemorizeClientOptions::from_env();

    tracing::info!("🧪 Memorize Integration Tests (Rust)");
    tracing::info!("   Server: {}", options.url);
    tracing::info!("   Auth: {}", if options.api_key.is_some() { "enabled" } else { "disabled" });
    println!();

    // Run all tests
    test_basic_operations().await?;
    test_delete_all().await?;
    test_keys_and_contains().await?;
    test_search_keys().await?;
    test_parallel_set_get().await?;
    test_data_isolation().await?;
    test_expiration().await?;

    println!();
    tracing::info!("✅ All tests passed!");

    Ok(())
}

/// Create a client from environment configuration
async fn create_client() -> Result<MemorizeClient> {
    let options = MemorizeClientOptions::from_env();
    let client = MemorizeClient::with_options(options).await?;
    Ok(client)
}

/// Test basic SET/GET/DELETE operations
async fn test_basic_operations() -> Result<()> {
    tracing::info!("Test: Basic Operations");

    let client = create_client().await?;

    // SET
    let key = format!("basic-test-{}", uuid::Uuid::new_v4());
    let value = "hello world";

    client.set(&key, value, Some(60)).await?;
    tracing::info!("   SET {} = {}", key, value);

    // GET
    let result = client.get(&key).await?;
    assert!(result.is_some(), "Key should be found");
    assert_eq!(result.as_deref(), Some(value), "Value should match");
    tracing::info!("   GET {} → {:?}", key, result);

    // DELETE
    let deleted = client.delete(&key).await?;
    assert!(deleted, "Key should be deleted");
    tracing::info!("   DELETE {} → {}", key, deleted);

    // GET after DELETE
    let result = client.get(&key).await?;
    assert!(result.is_none(), "Key should not be found after delete");
    tracing::info!("   GET {} → None (as expected)", key);

    tracing::info!("   ✓ Basic operations work correctly");
    Ok(())
}

/// Test DELETE_ALL operation
async fn test_delete_all() -> Result<()> {
    tracing::info!("Test: Delete All");

    let client = create_client().await?;
    let prefix = format!("delete-all-test-{}", uuid::Uuid::new_v4());

    // Create some test keys
    for i in 0..5 {
        let key = format!("{}:{}", prefix, i);
        client.set(&key, &format!("value{}", i), Some(300)).await?;
    }
    tracing::info!("   Created 5 test keys with prefix {}", prefix);

    // Verify keys exist
    let keys_before = client.keys(Some(&format!("{}:*", prefix)), None).await?;
    assert_eq!(keys_before.len(), 5, "Should have 5 keys");
    tracing::info!("   Verified {} keys exist", keys_before.len());

    // Delete all
    let deleted_count = client.delete_all().await?;
    tracing::info!("   DELETE_ALL → {} entries deleted", deleted_count);
    assert!(deleted_count >= 5, "Should have deleted at least 5 entries");

    // Verify keys are gone
    let keys_after = client.keys(Some(&format!("{}:*", prefix)), None).await?;
    assert_eq!(keys_after.len(), 0, "Should have no keys after delete_all");
    tracing::info!("   Verified all keys are gone");

    tracing::info!("   ✓ Delete All works correctly");
    Ok(())
}

/// Test KEYS and CONTAINS operations
async fn test_keys_and_contains() -> Result<()> {
    tracing::info!("Test: Keys and Contains");

    let client = create_client().await?;

    // Create some test keys with a unique prefix
    let prefix = format!("keys-test-{}", uuid::Uuid::new_v4());
    let keys = vec![
        format!("{}:a", prefix),
        format!("{}:b", prefix),
        format!("{}:c", prefix),
    ];

    for key in &keys {
        client.set(key, "value", Some(60)).await?;
    }
    tracing::info!("   Created {} test keys with prefix {}", keys.len(), prefix);

    // Test CONTAINS
    assert!(client.contains(&keys[0]).await?, "Key should exist");
    assert!(!client.contains("nonexistent-key-12345").await?, "Nonexistent key should not exist");
    tracing::info!("   ✓ CONTAINS works correctly");

    // Test KEYS with pattern
    let pattern = format!("{}:*", prefix);
    let found_keys = client.keys(Some(&pattern), None).await?;
    assert_eq!(found_keys.len(), 3, "Should find 3 keys matching pattern");
    tracing::info!("   KEYS({}) → {} keys", pattern, found_keys.len());

    // Cleanup
    for key in &keys {
        client.delete(key).await?;
    }

    tracing::info!("   ✓ Keys and Contains work correctly");
    Ok(())
}

/// Test parallel SET and GET operations - verify no data mixing
async fn test_parallel_set_get() -> Result<()> {
    tracing::info!("Test: Parallel SET/GET (500 concurrent operations)");

    let num_operations = 500;
    let start = Instant::now();

    // Create unique key-value pairs
    let test_data: Vec<(String, String)> = (0..num_operations)
        .map(|i| {
            let key = format!("parallel-test-{}-{}", i, uuid::Uuid::new_v4());
            let value = format!("value-{}-{}", i, uuid::Uuid::new_v4());
            (key, value)
        })
        .collect();

    // Parallel SET operations
    let set_futures: Vec<_> = test_data
        .iter()
        .map(|(key, value)| {
            let key = key.clone();
            let value = value.clone();
            async move {
                let client = create_client().await?;
                client.set(&key, &value, Some(300)).await?;
                Ok::<_, anyhow::Error>(())
            }
        })
        .collect();

    join_all(set_futures)
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;

    let set_elapsed = start.elapsed();
    tracing::info!("   SET {} keys in {:?}", num_operations, set_elapsed);

    // Parallel GET operations and verify values
    let get_start = Instant::now();
    let errors = Arc::new(AtomicUsize::new(0));

    let get_futures: Vec<_> = test_data
        .iter()
        .map(|(key, expected_value)| {
            let key = key.clone();
            let expected_value = expected_value.clone();
            let errors = Arc::clone(&errors);
            async move {
                let client = create_client().await?;
                let result = client.get(&key).await?;

                if result.is_none() {
                    tracing::error!("Key not found: {}", key);
                    errors.fetch_add(1, Ordering::SeqCst);
                } else if result.as_deref() != Some(expected_value.as_str()) {
                    tracing::error!(
                        "Value mismatch for key {}: expected '{}', got '{:?}'",
                        key,
                        expected_value,
                        result
                    );
                    errors.fetch_add(1, Ordering::SeqCst);
                }

                Ok::<_, anyhow::Error>(())
            }
        })
        .collect();

    join_all(get_futures)
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;

    let get_elapsed = get_start.elapsed();
    let error_count = errors.load(Ordering::SeqCst);

    tracing::info!("   GET {} keys in {:?}", num_operations, get_elapsed);
    tracing::info!(
        "   Throughput: {:.0} ops/sec (SET), {:.0} ops/sec (GET)",
        num_operations as f64 / set_elapsed.as_secs_f64(),
        num_operations as f64 / get_elapsed.as_secs_f64()
    );

    assert_eq!(error_count, 0, "No errors should occur");
    tracing::info!("   ✓ All {} values verified correctly", num_operations);

    Ok(())
}

/// Test that concurrent operations on different keys don't interfere
async fn test_data_isolation() -> Result<()> {
    tracing::info!("Test: Data Isolation (concurrent writes to different keys)");

    let num_clients = 50;
    let ops_per_client = 20;
    let semaphore = Arc::new(Semaphore::new(100)); // Limit concurrent connections

    let results = Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    let client_futures: Vec<_> = (0..num_clients)
        .map(|client_id| {
            let results = Arc::clone(&results);
            let semaphore = Arc::clone(&semaphore);

            async move {
                let _permit = semaphore.acquire().await?;
                let client = create_client().await?;

                for op in 0..ops_per_client {
                    let key = format!("isolation-client{}-op{}", client_id, op);
                    let value = format!("client{}-value{}-{}", client_id, op, uuid::Uuid::new_v4());

                    // Store what we're setting
                    {
                        let mut r = results.lock().await;
                        r.insert(key.clone(), value.clone());
                    }

                    // SET
                    client.set(&key, &value, Some(300)).await?;
                }

                Ok::<_, anyhow::Error>(())
            }
        })
        .collect();

    join_all(client_futures)
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;

    // Verify all values
    let expected = results.lock().await;
    let mut errors = 0;

    for (key, expected_value) in expected.iter() {
        let client = create_client().await?;
        let result = client.get(key).await?;

        if result.as_deref() != Some(expected_value.as_str()) {
            tracing::error!(
                "Isolation failure: key={}, expected={}, got={:?}",
                key,
                expected_value,
                result
            );
            errors += 1;
        }
    }

    assert_eq!(errors, 0, "No isolation failures should occur");
    tracing::info!(
        "   ✓ {} keys verified, no cross-contamination",
        expected.len()
    );

    Ok(())
}

/// Test TTL expiration
async fn test_expiration() -> Result<()> {
    tracing::info!("Test: TTL Expiration");

    let client = create_client().await?;

    let key = format!("expire-test-{}", uuid::Uuid::new_v4());

    // SET with 1 second TTL
    client.set(&key, "temporary", Some(1)).await?;

    // Should exist immediately
    let result = client.get(&key).await?;
    assert!(result.is_some(), "Key should exist immediately");
    tracing::info!("   SET {} with TTL=1s", key);

    // Wait for expiration
    tracing::info!("   Waiting 2 seconds for expiration...");
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Should be gone
    let result = client.get(&key).await?;
    assert!(result.is_none(), "Key should be expired after TTL");

    tracing::info!("   ✓ TTL expiration works correctly");
    Ok(())
}

/// Test search_keys with prefix matching and pagination
async fn test_search_keys() -> Result<()> {
    tracing::info!("Test: Search Keys (prefix matching and pagination)");

    let client = create_client().await?;

    // Clear any existing test keys first
    client.delete_all().await?;

    // Create test keys with different prefixes
    let prefix = format!("search-test-{}", uuid::Uuid::new_v4());
    let user_prefix = format!("{}:user", prefix);
    let session_prefix = format!("{}:session", prefix);

    // Create 15 user keys and 5 session keys
    for i in 0..15 {
        let key = format!("{}:{:02}", user_prefix, i);
        client.set(&key, &format!("user-value-{}", i), Some(300)).await?;
    }
    for i in 0..5 {
        let key = format!("{}:{}", session_prefix, i);
        client.set(&key, &format!("session-value-{}", i), Some(300)).await?;
    }
    tracing::info!("   Created 15 user keys and 5 session keys with prefix {}", prefix);

    // Test 1: Search for all user keys (should get first 50 by default, but we only have 15)
    let (keys, total) = client.search_keys(&user_prefix, None, None).await?;
    assert_eq!(total, 15, "Should find 15 user keys total");
    assert_eq!(keys.len(), 15, "Should return all 15 user keys");
    tracing::info!("   search_keys({}) → {} keys, total={}", user_prefix, keys.len(), total);

    // Test 2: Search for session keys
    let (keys, total) = client.search_keys(&session_prefix, None, None).await?;
    assert_eq!(total, 5, "Should find 5 session keys total");
    assert_eq!(keys.len(), 5, "Should return all 5 session keys");
    tracing::info!("   search_keys({}) → {} keys, total={}", session_prefix, keys.len(), total);

    // Test 3: Pagination - get first 5 user keys
    let (page1, total) = client.search_keys(&user_prefix, Some(5), Some(0)).await?;
    assert_eq!(total, 15, "Total should still be 15");
    assert_eq!(page1.len(), 5, "Should return 5 keys");
    tracing::info!("   Page 1 (limit=5, skip=0): {:?}", page1);

    // Test 4: Pagination - get next 5 user keys
    let (page2, _) = client.search_keys(&user_prefix, Some(5), Some(5)).await?;
    assert_eq!(page2.len(), 5, "Should return 5 keys");
    tracing::info!("   Page 2 (limit=5, skip=5): {:?}", page2);

    // Test 5: Pagination - get last 5 user keys
    let (page3, _) = client.search_keys(&user_prefix, Some(5), Some(10)).await?;
    assert_eq!(page3.len(), 5, "Should return 5 keys");
    tracing::info!("   Page 3 (limit=5, skip=10): {:?}", page3);

    // Verify pagination returns different keys and they're sorted
    assert!(page1[0] < page2[0], "Page 1 keys should come before page 2 keys");
    assert!(page2[0] < page3[0], "Page 2 keys should come before page 3 keys");

    // Test 6: Search with no matches
    let (keys, total) = client.search_keys("nonexistent-prefix-xyz", None, None).await?;
    assert_eq!(total, 0, "Should find no keys");
    assert!(keys.is_empty(), "Should return empty list");
    tracing::info!("   search_keys(nonexistent-prefix) → {} keys, total={}", keys.len(), total);

    // Cleanup
    client.delete_all().await?;

    tracing::info!("   ✓ Search Keys works correctly with prefix matching and pagination");
    Ok(())
}
