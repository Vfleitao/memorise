using System.Diagnostics;
using Memorize.Client;

/// <summary>
/// Integration tests for the Memorize cache server using the Memorize.Client library.
/// Demonstrates how simple the client is to use while thoroughly testing the server.
/// </summary>
public static class Program
{
    public static async Task Main(string[] args)
    {
        // Create client from environment variables (or defaults)
        var options = MemorizeClientOptions.FromEnvironment();
        
        Console.WriteLine("🧪 Memorize Integration Tests (C#)");
        Console.WriteLine($"   Server: {options.ServerAddress}");
        Console.WriteLine($"   Auth: {(string.IsNullOrEmpty(options.ApiKey) ? "disabled" : "enabled")}");
        Console.WriteLine();

        try
        {
            // All tests use the same client instance (thread-safe, connection reuse)
            using var cache = new MemorizeClient(options);

            await TestBasicOperations(cache);
            await TestJsonSerialization(cache);
            await TestCacheAsidePattern(cache);
            await TestParallelOperations(cache);
            await TestDataIsolation(cache);
            await TestExpiration(cache);

            Console.WriteLine();
            Console.WriteLine("✅ All tests passed!");
        }
        catch (Exception ex)
        {
            Console.ForegroundColor = ConsoleColor.Red;
            Console.WriteLine($"❌ Test failed: {ex.Message}");
            Console.ResetColor();
            Environment.Exit(1);
        }
    }

    /// <summary>
    /// Test basic SET, GET, DELETE operations
    /// </summary>
    private static async Task TestBasicOperations(MemorizeClient cache)
    {
        Console.WriteLine("Test: Basic Operations");

        var testKey = $"basic-test-{Guid.NewGuid()}";
        var testValue = "Hello from C#!";

        // SET
        await cache.SetAsync(testKey, testValue, ttlSeconds: 60);

        // GET
        var value = await cache.GetAsync(testKey);
        Debug.Assert(value != null, "GET should find the key");
        Debug.Assert(value == testValue, "GET should return correct value");

        // GET with default
        var missing = await cache.GetOrDefaultAsync("nonexistent-key", "default");
        Debug.Assert(missing == "default", "GetOrDefault should return default for missing key");

        // CONTAINS
        var exists = await cache.ContainsAsync(testKey);
        Debug.Assert(exists, "CONTAINS should return true");

        // KEYS
        var keys = await cache.GetKeysAsync();
        Debug.Assert(keys.Contains(testKey), "KEYS should include our key");

        // DELETE
        var deleted = await cache.DeleteAsync(testKey);
        Debug.Assert(deleted, "DELETE should succeed");

        // Verify deletion
        var afterDelete = await cache.GetAsync(testKey);
        Debug.Assert(afterDelete == null, "GET after DELETE should return null");

        Console.WriteLine("   ✓ Basic operations work correctly");
    }

    /// <summary>
    /// Test JSON serialization extension methods
    /// </summary>
    private static async Task TestJsonSerialization(MemorizeClient cache)
    {
        Console.WriteLine("Test: JSON Serialization");

        var testKey = $"json-test-{Guid.NewGuid()}";
        var user = new TestUser("Alice", "alice@example.com", 30);

        // SetJson
        await cache.SetJsonAsync(testKey, user, ttlSeconds: 60);

        // GetJson
        var loaded = await cache.GetJsonAsync<TestUser>(testKey);
        Debug.Assert(loaded != null, "GetJson should deserialize the object");
        Debug.Assert(loaded.Name == user.Name, "Name should match");
        Debug.Assert(loaded.Email == user.Email, "Email should match");
        Debug.Assert(loaded.Age == user.Age, "Age should match");

        // Cleanup
        await cache.DeleteAsync(testKey);

        Console.WriteLine("   ✓ JSON serialization works correctly");
    }

    /// <summary>
    /// Test GetOrSet cache-aside pattern
    /// </summary>
    private static async Task TestCacheAsidePattern(MemorizeClient cache)
    {
        Console.WriteLine("Test: Cache-Aside Pattern (GetOrSet)");

        var testKey = $"cache-aside-{Guid.NewGuid()}";
        var factoryCallCount = 0;

        // First call - factory should be invoked
        var value1 = await cache.GetOrSetAsync(
            testKey,
            async () =>
            {
                factoryCallCount++;
                await Task.Delay(10); // Simulate async work
                return "computed-value";
            },
            ttlSeconds: 60);

        Debug.Assert(value1 == "computed-value", "First call should return factory value");
        Debug.Assert(factoryCallCount == 1, "Factory should be called once");

        // Second call - should hit cache, factory not invoked
        var value2 = await cache.GetOrSetAsync(
            testKey,
            async () =>
            {
                factoryCallCount++;
                return "should-not-be-returned";
            },
            ttlSeconds: 60);

        Debug.Assert(value2 == "computed-value", "Second call should return cached value");
        Debug.Assert(factoryCallCount == 1, "Factory should NOT be called again");

        // Test JSON variant
        var jsonKey = $"cache-aside-json-{Guid.NewGuid()}";
        var product = await cache.GetOrSetJsonAsync(
            jsonKey,
            async () => new TestProduct("Widget", 19.99m),
            ttlSeconds: 60);

        Debug.Assert(product.Name == "Widget", "JSON cache-aside should work");

        // Cleanup
        await cache.DeleteAsync(testKey);
        await cache.DeleteAsync(jsonKey);

        Console.WriteLine("   ✓ Cache-aside pattern works correctly");
    }

    /// <summary>
    /// Test parallel SET/GET operations (500 concurrent operations)
    /// </summary>
    private static async Task TestParallelOperations(MemorizeClient cache)
    {
        Console.WriteLine("Test: Parallel Operations (500 concurrent)");

        const int operationCount = 500;
        var testData = Enumerable.Range(0, operationCount)
            .Select(i => (Key: $"parallel-{Guid.NewGuid()}", Value: Guid.NewGuid().ToString()))
            .ToList();

        // Parallel SET
        var setStopwatch = Stopwatch.StartNew();
        await Task.WhenAll(testData.Select(item => 
            cache.SetAsync(item.Key, item.Value, ttlSeconds: 300)));
        setStopwatch.Stop();
        Console.WriteLine($"   SET {operationCount} keys in {setStopwatch.ElapsedMilliseconds}ms");

        // Parallel GET and verify
        var getStopwatch = Stopwatch.StartNew();
        var verificationErrors = new List<string>();
        
        await Task.WhenAll(testData.Select(async item =>
        {
            var value = await cache.GetAsync(item.Key);
            if (value == null)
            {
                lock (verificationErrors) verificationErrors.Add($"Key '{item.Key}' not found");
            }
            else if (value != item.Value)
            {
                lock (verificationErrors) verificationErrors.Add($"Key '{item.Key}' has wrong value");
            }
        }));
        getStopwatch.Stop();
        Console.WriteLine($"   GET {operationCount} keys in {getStopwatch.ElapsedMilliseconds}ms");

        // Throughput
        var setThroughput = (int)(operationCount * 1000.0 / Math.Max(1, setStopwatch.ElapsedMilliseconds));
        var getThroughput = (int)(operationCount * 1000.0 / Math.Max(1, getStopwatch.ElapsedMilliseconds));
        Console.WriteLine($"   Throughput: {setThroughput} ops/sec (SET), {getThroughput} ops/sec (GET)");

        if (verificationErrors.Count > 0)
            throw new Exception($"Verification failed: {string.Join("; ", verificationErrors.Take(5))}");

        Console.WriteLine($"   ✓ All {operationCount} values verified correctly");

        // Cleanup
        await Task.WhenAll(testData.Select(item => cache.DeleteAsync(item.Key)));
    }

    /// <summary>
    /// Test data isolation - multiple concurrent "clients" writing different keys
    /// </summary>
    private static async Task TestDataIsolation(MemorizeClient cache)
    {
        Console.WriteLine("Test: Data Isolation (concurrent writes)");

        const int clientCount = 50;
        const int keysPerClient = 20;

        var clientData = Enumerable.Range(0, clientCount)
            .Select(clientId =>
            {
                var clientGuid = Guid.NewGuid().ToString();
                return Enumerable.Range(0, keysPerClient)
                    .Select(keyIndex => (
                        Key: $"isolation-{clientGuid}-{keyIndex}",
                        Value: $"client-{clientGuid}-value-{keyIndex}"
                    ))
                    .ToList();
            })
            .ToList();

        // All clients write in parallel
        await Task.WhenAll(clientData.SelectMany(clientKeys =>
            clientKeys.Select(item => cache.SetAsync(item.Key, item.Value, ttlSeconds: 300))));

        // All clients verify in parallel
        var verificationErrors = new List<string>();
        await Task.WhenAll(clientData.SelectMany(clientKeys =>
            clientKeys.Select(async item =>
            {
                var value = await cache.GetAsync(item.Key);
                if (value == null)
                {
                    lock (verificationErrors) verificationErrors.Add($"Key not found: {item.Key}");
                }
                else if (value != item.Value)
                {
                    lock (verificationErrors)
                        verificationErrors.Add($"DATA ISOLATION VIOLATION! {item.Key}: expected '{item.Value}', got '{value}'");
                }
            })));

        if (verificationErrors.Count > 0)
            throw new Exception($"Data isolation failed: {string.Join("; ", verificationErrors.Take(5))}");

        Console.WriteLine($"   ✓ {clientCount * keysPerClient} keys verified, no cross-contamination");

        // Cleanup
        await Task.WhenAll(clientData.SelectMany(clientKeys =>
            clientKeys.Select(item => cache.DeleteAsync(item.Key))));
    }

    /// <summary>
    /// Test TTL expiration
    /// </summary>
    private static async Task TestExpiration(MemorizeClient cache)
    {
        Console.WriteLine("Test: TTL Expiration");

        var testKey = $"expiration-{Guid.NewGuid()}";

        // SET with 1 second TTL
        await cache.SetAsync(testKey, "expires-soon", ttlSeconds: 1);

        // Should exist immediately
        var immediate = await cache.GetAsync(testKey);
        Debug.Assert(immediate != null, "Key should exist immediately");

        // Wait for expiration
        Console.WriteLine("   Waiting 2 seconds for expiration...");
        await Task.Delay(TimeSpan.FromSeconds(2));

        // Should be expired
        var expired = await cache.GetAsync(testKey);
        Debug.Assert(expired == null, "Key should be expired after TTL");

        Console.WriteLine("   ✓ TTL expiration works correctly");
    }
}

// Test record types
record TestUser(string Name, string Email, int Age);
record TestProduct(string Name, decimal Price);
