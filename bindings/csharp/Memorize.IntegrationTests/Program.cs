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
            await TestDeleteAll(cache);
            await TestSearchKeys(cache);
            await TestJsonSerialization(cache);
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
    /// Test DELETE_ALL operation
    /// </summary>
    private static async Task TestDeleteAll(MemorizeClient cache)
    {
        Console.WriteLine("Test: Delete All");

        var prefix = $"delete-all-test-{Guid.NewGuid()}";

        // Create some test keys
        for (int i = 0; i < 5; i++)
        {
            await cache.SetAsync($"{prefix}:{i}", $"value{i}", ttlSeconds: 300);
        }
        Console.WriteLine($"   Created 5 test keys with prefix {prefix}");

        // Verify keys exist
        var keysBefore = await cache.GetKeysAsync();
        var matchingBefore = keysBefore.Count(k => k.StartsWith(prefix));
        Debug.Assert(matchingBefore == 5, $"Should have 5 keys, found {matchingBefore}");
        Console.WriteLine($"   Verified {matchingBefore} keys exist");

        // Delete all
        var deletedCount = await cache.DeleteAllAsync();
        Console.WriteLine($"   DELETE_ALL → {deletedCount} entries deleted");
        Debug.Assert(deletedCount >= 5, $"Should have deleted at least 5 entries, deleted {deletedCount}");

        // Verify keys are gone
        var keysAfter = await cache.GetKeysAsync();
        var matchingAfter = keysAfter.Count(k => k.StartsWith(prefix));
        Debug.Assert(matchingAfter == 0, $"Should have no keys after delete_all, found {matchingAfter}");
        Console.WriteLine("   Verified all keys are gone");

        Console.WriteLine("   ✓ Delete All works correctly");
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

    /// <summary>
    /// Test SearchKeys with prefix matching and pagination
    /// </summary>
    private static async Task TestSearchKeys(MemorizeClient cache)
    {
        Console.WriteLine("Test: Search Keys (prefix matching and pagination)");

        // Clear any existing keys first
        await cache.DeleteAllAsync();

        // Create test keys with different prefixes
        var prefix = $"search-test-{Guid.NewGuid()}";
        var userPrefix = $"{prefix}:user";
        var sessionPrefix = $"{prefix}:session";

        // Create 15 user keys and 5 session keys
        for (int i = 0; i < 15; i++)
        {
            await cache.SetAsync($"{userPrefix}:{i:D2}", $"user-value-{i}", ttlSeconds: 300);
        }
        for (int i = 0; i < 5; i++)
        {
            await cache.SetAsync($"{sessionPrefix}:{i}", $"session-value-{i}", ttlSeconds: 300);
        }
        Console.WriteLine($"   Created 15 user keys and 5 session keys with prefix {prefix}");

        // Test 1: Search for all user keys (should get default limit of 50, but we only have 15)
        var (keys, total) = await cache.SearchKeysAsync(userPrefix);
        Debug.Assert(total == 15, $"Should find 15 user keys total, found {total}");
        Debug.Assert(keys.Count == 15, $"Should return all 15 user keys, got {keys.Count}");
        Console.WriteLine($"   SearchKeys({userPrefix}) → {keys.Count} keys, total={total}");

        // Test 2: Search for session keys
        (keys, total) = await cache.SearchKeysAsync(sessionPrefix);
        Debug.Assert(total == 5, $"Should find 5 session keys total, found {total}");
        Debug.Assert(keys.Count == 5, $"Should return all 5 session keys, got {keys.Count}");
        Console.WriteLine($"   SearchKeys({sessionPrefix}) → {keys.Count} keys, total={total}");

        // Test 3: Pagination - get first 5 user keys
        var (page1, totalPage1) = await cache.SearchKeysAsync(userPrefix, limit: 5, skip: 0);
        Debug.Assert(totalPage1 == 15, $"Total should still be 15, got {totalPage1}");
        Debug.Assert(page1.Count == 5, $"Should return 5 keys, got {page1.Count}");
        Console.WriteLine($"   Page 1 (limit=5, skip=0): [{string.Join(", ", page1)}]");

        // Test 4: Pagination - get next 5 user keys
        var (page2, _) = await cache.SearchKeysAsync(userPrefix, limit: 5, skip: 5);
        Debug.Assert(page2.Count == 5, $"Should return 5 keys, got {page2.Count}");
        Console.WriteLine($"   Page 2 (limit=5, skip=5): [{string.Join(", ", page2)}]");

        // Test 5: Pagination - get last 5 user keys
        var (page3, _) = await cache.SearchKeysAsync(userPrefix, limit: 5, skip: 10);
        Debug.Assert(page3.Count == 5, $"Should return 5 keys, got {page3.Count}");
        Console.WriteLine($"   Page 3 (limit=5, skip=10): [{string.Join(", ", page3)}]");

        // Verify pagination returns different keys and they're sorted
        Debug.Assert(string.Compare(page1[0], page2[0]) < 0, "Page 1 keys should come before page 2 keys");
        Debug.Assert(string.Compare(page2[0], page3[0]) < 0, "Page 2 keys should come before page 3 keys");

        // Test 6: Search with no matches
        (keys, total) = await cache.SearchKeysAsync("nonexistent-prefix-xyz");
        Debug.Assert(total == 0, $"Should find no keys, found {total}");
        Debug.Assert(keys.Count == 0, $"Should return empty list, got {keys.Count}");
        Console.WriteLine($"   SearchKeys(nonexistent-prefix) → {keys.Count} keys, total={total}");

        // Cleanup
        await cache.DeleteAllAsync();

        Console.WriteLine("   ✓ Search Keys works correctly with prefix matching and pagination");
    }
}

// Test record types
record TestUser(string Name, string Email, int Age);
record TestProduct(string Name, decimal Price);
