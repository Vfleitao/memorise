using BenchmarkDotNet.Attributes;
using Memorize.Client;

namespace Memorize.Benchmarks;

/// <summary>
/// Benchmarks for Memorize cache operations.
/// Run with: dotnet run -c Release
/// Quick test: dotnet run -c Release -- --quick
/// </summary>
[MemoryDiagnoser]
[RankColumn]
public class MemorizeBenchmarks
{
    private MemorizeClient _client = null!;
    private string _testKey = null!;
    private string _testValue = null!;
    private string[] _batchKeys = null!;
    private string[] _searchKeys = null!;
    
    [Params(100, 1000)]
    public int PayloadSize { get; set; }
    
    [GlobalSetup]
    public async Task Setup()
    {
        var serverUrl = Environment.GetEnvironmentVariable("MEMORIZE_URL") ?? "http://localhost:50051";
        var apiKey = Environment.GetEnvironmentVariable("MEMORIZE_API_KEY");
        
        _client = apiKey is not null 
            ? new MemorizeClient(serverUrl, apiKey) 
            : new MemorizeClient(serverUrl);
        
        // Generate test data
        _testKey = $"bench:{Guid.NewGuid()}";
        _testValue = new string('x', PayloadSize);
        
        // Setup batch keys
        _batchKeys = Enumerable.Range(0, 100)
            .Select(i => $"batch:{Guid.NewGuid()}:{i}")
            .ToArray();
        
        // Setup search keys
        var searchPrefix = $"search:{Guid.NewGuid()}";
        _searchKeys = Enumerable.Range(0, 50)
            .Select(i => $"{searchPrefix}:{i:D4}")
            .ToArray();
        
        foreach (var key in _searchKeys)
        {
            await _client.SetAsync(key, _testValue);
        }
        
        // Pre-populate a key for GET benchmarks
        await _client.SetAsync(_testKey, _testValue);
    }
    
    [GlobalCleanup]
    public async Task Cleanup()
    {
        // Clean up test keys
        try
        {
            await _client.DeleteAsync(_testKey);
            foreach (var key in _batchKeys)
            {
                await _client.DeleteAsync(key);
            }
            foreach (var key in _searchKeys)
            {
                await _client.DeleteAsync(key);
            }
        }
        catch
        {
            // Ignore cleanup errors
        }
        
        _client.Dispose();
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Single Operations
    // ═══════════════════════════════════════════════════════════════
    
    [Benchmark(Description = "SET (single)")]
    public async Task Set_Single()
    {
        await _client.SetAsync(_testKey, _testValue);
    }
    
    [Benchmark(Description = "GET (single)")]
    public async Task<string?> Get_Single()
    {
        return await _client.GetAsync(_testKey);
    }
    
    [Benchmark(Description = "SET+GET (roundtrip)")]
    public async Task<string?> Set_Get_Roundtrip()
    {
        var key = $"rt:{Guid.NewGuid()}";
        await _client.SetAsync(key, _testValue);
        var result = await _client.GetAsync(key);
        await _client.DeleteAsync(key);
        return result;
    }
    
    [Benchmark(Description = "CONTAINS (existing)")]
    public async Task<bool> Contains_Existing()
    {
        return await _client.ContainsAsync(_testKey);
    }
    
    [Benchmark(Description = "CONTAINS (missing)")]
    public async Task<bool> Contains_Missing()
    {
        return await _client.ContainsAsync("nonexistent:key:12345");
    }
    
    [Benchmark(Description = "DELETE (single)")]
    public async Task<bool> Delete_Single()
    {
        var key = $"del:{Guid.NewGuid()}";
        await _client.SetAsync(key, _testValue);
        return await _client.DeleteAsync(key);
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Search Operations
    // ═══════════════════════════════════════════════════════════════
    
    [Benchmark(Description = "SEARCH_KEYS (50 results)")]
    public async Task<(IReadOnlyList<string> Keys, ulong TotalCount)> SearchKeys_50()
    {
        var prefix = _searchKeys[0][..^5]; // Remove the ":0000" suffix
        return await _client.SearchKeysAsync(prefix, limit: 50);
    }
    
    [Benchmark(Description = "SEARCH_KEYS (paginated 10)")]
    public async Task<(IReadOnlyList<string> Keys, ulong TotalCount)> SearchKeys_Paginated()
    {
        var prefix = _searchKeys[0][..^5];
        return await _client.SearchKeysAsync(prefix, limit: 10, skip: 20);
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Concurrent Operations
    // ═══════════════════════════════════════════════════════════════
    
    [Benchmark(Description = "SET (100 concurrent)")]
    public async Task Set_Concurrent_100()
    {
        var tasks = _batchKeys.Select((key, i) => 
            _client.SetAsync(key, _testValue));
        await Task.WhenAll(tasks);
    }
    
    [Benchmark(Description = "GET (100 concurrent)")]
    public async Task Get_Concurrent_100()
    {
        // First ensure keys exist
        var setTasks = _batchKeys.Select(key => _client.SetAsync(key, _testValue));
        await Task.WhenAll(setTasks);
        
        // Then benchmark GET
        var getTasks = _batchKeys.Select(key => _client.GetAsync(key));
        await Task.WhenAll(getTasks);
    }
    
    [Benchmark(Description = "Mixed SET/GET (100 concurrent)")]
    public async Task Mixed_Concurrent_100()
    {
        var tasks = new List<Task>();
        for (int i = 0; i < _batchKeys.Length; i++)
        {
            if (i % 2 == 0)
                tasks.Add(_client.SetAsync(_batchKeys[i], _testValue));
            else
                tasks.Add(_client.GetAsync(_batchKeys[i]));
        }
        await Task.WhenAll(tasks);
    }
}
