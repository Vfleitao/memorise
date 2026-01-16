use anyhow::Result;
use futures::future::join_all;
use memorize_proto::memorize_client::MemorizeClient;
use memorize_proto::{GetRequest, SetRequest};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const SERVER_URL: &str = "http://127.0.0.1:50051";

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

    tracing::info!("🧪 Memorize Integration Tests");
    tracing::info!("   Server: {}", SERVER_URL);
    println!();

    // Run all tests
    test_basic_operations().await?;
    test_parallel_set_get().await?;
    test_data_isolation().await?;
    test_expiration().await?;
    test_streaming_execute().await?;

    println!();
    tracing::info!("✅ All tests passed!");

    Ok(())
}

/// Test basic SET/GET/DELETE operations
async fn test_basic_operations() -> Result<()> {
    tracing::info!("Test: Basic Operations");

    let mut client = MemorizeClient::connect(SERVER_URL).await?;

    // SET
    let key = format!("basic-test-{}", uuid::Uuid::new_v4());
    let value = "hello world";

    client
        .set(SetRequest {
            key: key.clone(),
            value: value.to_string(),
            ttl_seconds: 60,
        })
        .await?;

    // GET
    let response = client.get(GetRequest { key: key.clone() }).await?;
    let get_response = response.into_inner();

    assert!(
        get_response.value.is_some(),
        "Key should be found"
    );
    assert_eq!(
        get_response.value.as_deref(),
        Some(value),
        "Value should match"
    );

    // DELETE
    let delete_response = client
        .delete(memorize_proto::DeleteRequest { key: key.clone() })
        .await?
        .into_inner();

    assert!(delete_response.deleted, "Key should be deleted");

    // GET after DELETE
    let response = client.get(GetRequest { key }).await?;
    assert!(response.into_inner().value.is_none(), "Key should not be found after delete");

    tracing::info!("   ✓ Basic operations work correctly");
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
                let mut client = MemorizeClient::connect(SERVER_URL).await?;
                client
                    .set(SetRequest {
                        key,
                        value,
                        ttl_seconds: 300,
                    })
                    .await?;
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
                let mut client = MemorizeClient::connect(SERVER_URL).await?;
                let response = client.get(GetRequest { key: key.clone() }).await?;
                let get_response = response.into_inner();

                if get_response.value.is_none() {
                    tracing::error!("Key not found: {}", key);
                    errors.fetch_add(1, Ordering::SeqCst);
                } else if get_response.value.as_deref() != Some(expected_value.as_str()) {
                    tracing::error!(
                        "Value mismatch for key {}: expected '{}', got '{:?}'",
                        key,
                        expected_value,
                        get_response.value
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
                let mut client = MemorizeClient::connect(SERVER_URL).await?;

                for op in 0..ops_per_client {
                    let key = format!("isolation-client{}-op{}", client_id, op);
                    let value = format!("client{}-value{}-{}", client_id, op, uuid::Uuid::new_v4());

                    // Store what we're setting
                    {
                        let mut r = results.lock().await;
                        r.insert(key.clone(), value.clone());
                    }

                    // SET
                    client
                        .set(SetRequest {
                            key,
                            value,
                            ttl_seconds: 300,
                        })
                        .await?;
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
        let mut client = MemorizeClient::connect(SERVER_URL).await?;
        let response = client
            .get(GetRequest {
                key: key.clone(),
            })
            .await?
            .into_inner();

        if response.value.as_deref() != Some(expected_value.as_str()) {
            tracing::error!(
                "Isolation failure: key={}, expected={}, got={:?}",
                key,
                expected_value,
                response.value
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

    let mut client = MemorizeClient::connect(SERVER_URL).await?;

    let key = format!("expire-test-{}", uuid::Uuid::new_v4());

    // SET with 1 second TTL
    client
        .set(SetRequest {
            key: key.clone(),
            value: "temporary".to_string(),
            ttl_seconds: 1,
        })
        .await?;

    // Should exist immediately
    let response = client.get(GetRequest { key: key.clone() }).await?;
    assert!(response.into_inner().value.is_some(), "Key should exist immediately");

    // Wait for expiration
    tracing::info!("   Waiting 2 seconds for expiration...");
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Should be gone
    let response = client.get(GetRequest { key }).await?;
    assert!(
        response.into_inner().value.is_none(),
        "Key should be expired after TTL"
    );

    tracing::info!("   ✓ TTL expiration works correctly");
    Ok(())
}

/// Test bi-directional streaming Execute RPC
async fn test_streaming_execute() -> Result<()> {
    use futures::StreamExt;
    use memorize_proto::{command, Command, ContainsRequest, DeleteRequest, GetRequest, SetRequest};

    tracing::info!("Test: Streaming Execute (bi-directional)");

    let mut client = MemorizeClient::connect(SERVER_URL).await?;

    // Create a stream of commands
    let test_key = format!("stream-test-{}", uuid::Uuid::new_v4());
    let test_value = "streamed-value";

    let commands = vec![
        Command {
            id: "cmd-1".to_string(),
            command: Some(command::Command::Set(SetRequest {
                key: test_key.clone(),
                value: test_value.to_string(),
                ttl_seconds: 60,
            })),
        },
        Command {
            id: "cmd-2".to_string(),
            command: Some(command::Command::Get(GetRequest {
                key: test_key.clone(),
            })),
        },
        Command {
            id: "cmd-3".to_string(),
            command: Some(command::Command::Contains(ContainsRequest {
                key: test_key.clone(),
            })),
        },
        Command {
            id: "cmd-4".to_string(),
            command: Some(command::Command::Delete(DeleteRequest {
                key: test_key.clone(),
            })),
        },
        Command {
            id: "cmd-5".to_string(),
            command: Some(command::Command::Get(GetRequest {
                key: test_key.clone(),
            })),
        },
    ];

    let command_stream = tokio_stream::iter(commands);

    // Send commands and collect responses
    let response = client.execute(command_stream).await?;
    let mut response_stream = response.into_inner();

    let mut responses = HashMap::new();
    while let Some(result) = response_stream.next().await {
        let cmd_response = result?;
        responses.insert(cmd_response.id.clone(), cmd_response);
    }

    // Verify responses
    assert_eq!(responses.len(), 5, "Should receive 5 responses");

    // cmd-1: SET should succeed
    let set_resp = responses.get("cmd-1").expect("cmd-1 response");
    if let Some(memorize_proto::command_response::Response::Set(r)) = &set_resp.response {
        assert!(r.success, "SET should succeed");
    } else {
        panic!("cmd-1 should be SET response");
    }

    // cmd-2: GET should find the value
    let get_resp = responses.get("cmd-2").expect("cmd-2 response");
    if let Some(memorize_proto::command_response::Response::Get(r)) = &get_resp.response {
        assert!(r.value.is_some(), "GET should find key");
        assert_eq!(r.value.as_deref(), Some(test_value), "Value should match");
    } else {
        panic!("cmd-2 should be GET response");
    }

    // cmd-3: CONTAINS should be true
    let contains_resp = responses.get("cmd-3").expect("cmd-3 response");
    if let Some(memorize_proto::command_response::Response::Contains(r)) = &contains_resp.response {
        assert!(r.exists, "CONTAINS should be true");
    } else {
        panic!("cmd-3 should be CONTAINS response");
    }

    // cmd-4: DELETE should succeed
    let delete_resp = responses.get("cmd-4").expect("cmd-4 response");
    if let Some(memorize_proto::command_response::Response::Delete(r)) = &delete_resp.response {
        assert!(r.deleted, "DELETE should succeed");
    } else {
        panic!("cmd-4 should be DELETE response");
    }

    // cmd-5: GET after delete should not find
    let get_resp2 = responses.get("cmd-5").expect("cmd-5 response");
    if let Some(memorize_proto::command_response::Response::Get(r)) = &get_resp2.response {
        assert!(r.value.is_none(), "GET after DELETE should not find key");
    } else {
        panic!("cmd-5 should be GET response");
    }

    tracing::info!("   ✓ Streaming Execute works correctly with command correlation");
    Ok(())
}
