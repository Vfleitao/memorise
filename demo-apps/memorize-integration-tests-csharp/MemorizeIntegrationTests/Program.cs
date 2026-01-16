using System.Diagnostics;
using Grpc.Core;
using Grpc.Net.Client;
using Memorize;

namespace MemorizeIntegrationTests;

/// <summary>
/// Integration tests for the Memorize gRPC server.
/// This C# implementation mirrors the Rust integration tests exactly.
/// </summary>
public static class Program
{
    private static readonly string ServerUrl = 
        Environment.GetEnvironmentVariable("MEMORIZE_SERVER_URL") ?? "http://127.0.0.1:50051";

    public static async Task Main(string[] args)
    {
        Console.WriteLine("🧪 Memorize Integration Tests (C#)");
        Console.WriteLine($"   Server: {ServerUrl}");
        Console.WriteLine();

        try
        {
            await TestBasicOperations();
            await TestParallelSetGet();
            await TestDataIsolation();
            await TestExpiration();
            await TestStreamingExecute();

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
    private static async Task TestBasicOperations()
    {
        Console.WriteLine("Test: Basic Operations");

        using var channel = GrpcChannel.ForAddress(ServerUrl);
        var client = new Memorize.Memorize.MemorizeClient(channel);

        var testKey = $"basic-test-{Guid.NewGuid()}";
        var testValue = "Hello from C#!";

        // SET
        var setResponse = await client.SetAsync(new SetRequest
        {
            Key = testKey,
            Value = testValue,
            TtlSeconds = 60
        });
        Debug.Assert(setResponse.Success, "SET should succeed");

        // GET
        var getResponse = await client.GetAsync(new GetRequest { Key = testKey });
        Debug.Assert(getResponse.Found, "GET should find the key");
        Debug.Assert(getResponse.Value == testValue, "GET should return correct value");

        // CONTAINS
        var containsResponse = await client.ContainsAsync(new ContainsRequest { Key = testKey });
        Debug.Assert(containsResponse.Exists, "CONTAINS should return true");

        // KEYS
        var keysResponse = await client.KeysAsync(new KeysRequest());
        Debug.Assert(keysResponse.Keys.Contains(testKey), "KEYS should include our key");

        // DELETE
        var deleteResponse = await client.DeleteAsync(new DeleteRequest { Key = testKey });
        Debug.Assert(deleteResponse.Deleted, "DELETE should succeed");

        // Verify deletion
        var getAfterDelete = await client.GetAsync(new GetRequest { Key = testKey });
        Debug.Assert(!getAfterDelete.Found, "GET after DELETE should not find key");

        Console.WriteLine("   ✓ Basic operations work correctly");
    }

    /// <summary>
    /// Test parallel SET/GET operations (500 concurrent operations)
    /// </summary>
    private static async Task TestParallelSetGet()
    {
        Console.WriteLine("Test: Parallel SET/GET (500 concurrent operations)");

        using var channel = GrpcChannel.ForAddress(ServerUrl);
        var client = new Memorize.Memorize.MemorizeClient(channel);

        const int operationCount = 500;
        var testData = Enumerable.Range(0, operationCount)
            .Select(i => (Key: $"parallel-test-{Guid.NewGuid()}", Value: Guid.NewGuid().ToString()))
            .ToList();

        // Parallel SET
        var setStopwatch = Stopwatch.StartNew();
        var setTasks = testData.Select(async item =>
        {
            await client.SetAsync(new SetRequest
            {
                Key = item.Key,
                Value = item.Value,
                TtlSeconds = 300
            });
        });
        await Task.WhenAll(setTasks);
        setStopwatch.Stop();
        Console.WriteLine($"   SET {operationCount} keys in {setStopwatch.ElapsedMilliseconds}ms");

        // Parallel GET and verify
        var getStopwatch = Stopwatch.StartNew();
        var verificationErrors = new List<string>();
        var getTasks = testData.Select(async item =>
        {
            var response = await client.GetAsync(new GetRequest { Key = item.Key });
            if (!response.Found)
            {
                lock (verificationErrors)
                    verificationErrors.Add($"Key '{item.Key}' not found");
            }
            else if (response.Value != item.Value)
            {
                lock (verificationErrors)
                    verificationErrors.Add($"Key '{item.Key}' has wrong value");
            }
        });
        await Task.WhenAll(getTasks);
        getStopwatch.Stop();
        Console.WriteLine($"   GET {operationCount} keys in {getStopwatch.ElapsedMilliseconds}ms");

        // Calculate throughput
        var setThroughput = (int)(operationCount * 1000.0 / setStopwatch.ElapsedMilliseconds);
        var getThroughput = (int)(operationCount * 1000.0 / getStopwatch.ElapsedMilliseconds);
        Console.WriteLine($"   Throughput: {setThroughput} ops/sec (SET), {getThroughput} ops/sec (GET)");

        if (verificationErrors.Count > 0)
        {
            throw new Exception($"Verification failed: {string.Join("; ", verificationErrors.Take(5))}");
        }

        Console.WriteLine($"   ✓ All {operationCount} values verified correctly");

        // Cleanup
        var deleteTasks = testData.Select(async item => await client.DeleteAsync(new DeleteRequest { Key = item.Key }));
        await Task.WhenAll(deleteTasks);
    }

    /// <summary>
    /// Test data isolation - multiple concurrent "clients" writing different keys
    /// Proves that we never get cross-contamination (key A returning value from key B)
    /// </summary>
    private static async Task TestDataIsolation()
    {
        Console.WriteLine("Test: Data Isolation (concurrent writes to different keys)");

        using var channel = GrpcChannel.ForAddress(ServerUrl);
        var client = new Memorize.Memorize.MemorizeClient(channel);

        const int clientCount = 50;
        const int keysPerClient = 20;

        // Each simulated client has a unique ID and writes keys with that prefix
        var clientData = Enumerable.Range(0, clientCount)
            .Select(clientId => 
            {
                var clientGuid = Guid.NewGuid().ToString();
                return Enumerable.Range(0, keysPerClient)
                    .Select(keyIndex => (
                        ClientId: clientGuid,
                        Key: $"isolation-{clientGuid}-{keyIndex}",
                        Value: $"client-{clientGuid}-value-{keyIndex}"
                    ))
                    .ToList();
            })
            .ToList();

        // All clients write their keys in parallel
        var writeTasks = clientData.SelectMany(clientKeys =>
            clientKeys.Select(async item =>
            {
                await client.SetAsync(new SetRequest
                {
                    Key = item.Key,
                    Value = item.Value,
                    TtlSeconds = 300
                });
            }));
        await Task.WhenAll(writeTasks);

        // All clients read and verify their keys in parallel
        var verificationErrors = new List<string>();
        var readTasks = clientData.SelectMany(clientKeys =>
            clientKeys.Select(async item =>
            {
                var response = await client.GetAsync(new GetRequest { Key = item.Key });
                if (!response.Found)
                {
                    lock (verificationErrors)
                        verificationErrors.Add($"Key '{item.Key}' not found");
                }
                else
                {
                    var actualValue = response.Value;
                    if (actualValue != item.Value)
                    {
                        lock (verificationErrors)
                            verificationErrors.Add(
                                $"DATA ISOLATION VIOLATION! Key '{item.Key}' expected '{item.Value}' but got '{actualValue}'");
                    }
                }
            }));
        await Task.WhenAll(readTasks);

        var totalKeys = clientCount * keysPerClient;
        if (verificationErrors.Count > 0)
        {
            throw new Exception($"Data isolation test failed: {string.Join("; ", verificationErrors.Take(5))}");
        }

        Console.WriteLine($"   ✓ {totalKeys} keys verified, no cross-contamination");

        // Cleanup
        var deleteTasks = clientData.SelectMany(clientKeys =>
            clientKeys.Select(async item => await client.DeleteAsync(new DeleteRequest { Key = item.Key })));
        await Task.WhenAll(deleteTasks);
    }

    /// <summary>
    /// Test TTL expiration
    /// </summary>
    private static async Task TestExpiration()
    {
        Console.WriteLine("Test: TTL Expiration");

        using var channel = GrpcChannel.ForAddress(ServerUrl);
        var client = new Memorize.Memorize.MemorizeClient(channel);

        var testKey = $"expiration-test-{Guid.NewGuid()}";

        // SET with 1 second TTL
        await client.SetAsync(new SetRequest
        {
            Key = testKey,
            Value = "expires-soon",
            TtlSeconds = 1
        });

        // Should exist immediately
        var immediateGet = await client.GetAsync(new GetRequest { Key = testKey });
        Debug.Assert(immediateGet.Found, "Key should exist immediately after SET");

        // Wait for expiration
        Console.WriteLine("   Waiting 2 seconds for expiration...");
        await Task.Delay(TimeSpan.FromSeconds(2));

        // Should be expired now
        var expiredGet = await client.GetAsync(new GetRequest { Key = testKey });
        Debug.Assert(!expiredGet.Found, "Key should be expired after TTL");

        Console.WriteLine("   ✓ TTL expiration works correctly");
    }

    /// <summary>
    /// Test streaming Execute RPC (bi-directional streaming)
    /// </summary>
    private static async Task TestStreamingExecute()
    {
        Console.WriteLine("Test: Streaming Execute (bi-directional)");

        using var channel = GrpcChannel.ForAddress(ServerUrl);
        var client = new Memorize.Memorize.MemorizeClient(channel);

        using var call = client.Execute();

        var testKey = $"stream-test-{Guid.NewGuid()}";
        var testValue = "streaming-value";

        // Send SET command
        var setCommandId = Guid.NewGuid().ToString();
        await call.RequestStream.WriteAsync(new Command
        {
            Id = setCommandId,
            Set = new SetRequest
            {
                Key = testKey,
                Value = testValue,
                TtlSeconds = 60
            }
        });

        // Send GET command
        var getCommandId = Guid.NewGuid().ToString();
        await call.RequestStream.WriteAsync(new Command
        {
            Id = getCommandId,
            Get = new GetRequest { Key = testKey }
        });

        // Send DELETE command
        var deleteCommandId = Guid.NewGuid().ToString();
        await call.RequestStream.WriteAsync(new Command
        {
            Id = deleteCommandId,
            Delete = new DeleteRequest { Key = testKey }
        });

        // Complete sending
        await call.RequestStream.CompleteAsync();

        // Collect responses
        var responses = new Dictionary<string, CommandResponse>();
        await foreach (var response in call.ResponseStream.ReadAllAsync())
        {
            responses[response.Id] = response;
        }

        // Verify responses
        Debug.Assert(responses.ContainsKey(setCommandId), "Should receive SET response");
        Debug.Assert(responses[setCommandId].Set?.Success == true, "SET should succeed");

        Debug.Assert(responses.ContainsKey(getCommandId), "Should receive GET response");
        Debug.Assert(responses[getCommandId].Get?.Found == true, "GET should find the key");
        Debug.Assert(responses[getCommandId].Get?.Value == testValue, "GET should return correct value");

        Debug.Assert(responses.ContainsKey(deleteCommandId), "Should receive DELETE response");
        Debug.Assert(responses[deleteCommandId].Delete?.Deleted == true, "DELETE should succeed");

        Console.WriteLine("   ✓ Streaming Execute works correctly with command correlation");
    }
}
