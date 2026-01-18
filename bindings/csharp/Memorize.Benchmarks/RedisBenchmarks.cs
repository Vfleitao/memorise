using BenchmarkDotNet.Attributes;
using StackExchange.Redis;

namespace Memorize.Benchmarks;

/// <summary>
/// Benchmarks for Redis cache operations (for comparison with Memorize).
/// Requires Redis server running locally on port 6379.
/// </summary>
[MemoryDiagnoser]
[RankColumn]
public class RedisBenchmarks
{
    private ConnectionMultiplexer _redis = null!;
    private IDatabase _db = null!;
    private string _testKey = null!;
    private string _testValue = null!;
    private string[] _batchKeys = null!;
    private string[] _searchKeys = null!;
    
    [Params(100, 1000)]
    public int PayloadSize { get; set; }
    
    [GlobalSetup]
    public async Task Setup()
    {
        var redisHost = Environment.GetEnvironmentVariable("REDIS_HOST") ?? "localhost:6379";
        
        _redis = await ConnectionMultiplexer.ConnectAsync(redisHost);
        _db = _redis.GetDatabase();
        
        // Generate test data
        _testKey = $"bench:{Guid.NewGuid()}";
        _testValue = new string('x', PayloadSize);
        
        // Setup batch keys
        _batchKeys = Enumerable.Range(0, 100)
            .Select(i => $"batch:{Guid.NewGuid()}:{i}")
            .ToArray();
        
        // Setup search keys (Redis SCAN pattern)
        var searchPrefix = $"search:{Guid.NewGuid()}";
        _searchKeys = Enumerable.Range(0, 50)
            .Select(i => $"{searchPrefix}:{i:D4}")
            .ToArray();
        
        foreach (var key in _searchKeys)
        {
            await _db.StringSetAsync(key, _testValue);
        }
        
        // Pre-populate a key for GET benchmarks
        await _db.StringSetAsync(_testKey, _testValue);
    }
    
    [GlobalCleanup]
    public async Task Cleanup()
    {
        // Clean up test keys
        try
        {
            await _db.KeyDeleteAsync(_testKey);
            foreach (var key in _batchKeys)
            {
                await _db.KeyDeleteAsync(key);
            }
            foreach (var key in _searchKeys)
            {
                await _db.KeyDeleteAsync(key);
            }
        }
        catch
        {
            // Ignore cleanup errors
        }
        
        _redis.Dispose();
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Single Operations
    // ═══════════════════════════════════════════════════════════════
    
    [Benchmark(Description = "SET (single)")]
    public async Task Set_Single()
    {
        await _db.StringSetAsync(_testKey, _testValue);
    }
    
    [Benchmark(Description = "GET (single)")]
    public async Task<RedisValue> Get_Single()
    {
        return await _db.StringGetAsync(_testKey);
    }
    
    [Benchmark(Description = "SET+GET (roundtrip)")]
    public async Task<RedisValue> Set_Get_Roundtrip()
    {
        var key = $"rt:{Guid.NewGuid()}";
        await _db.StringSetAsync(key, _testValue);
        var result = await _db.StringGetAsync(key);
        await _db.KeyDeleteAsync(key);
        return result;
    }
    
    [Benchmark(Description = "EXISTS (existing)")]
    public async Task<bool> Exists_Existing()
    {
        return await _db.KeyExistsAsync(_testKey);
    }
    
    [Benchmark(Description = "EXISTS (missing)")]
    public async Task<bool> Exists_Missing()
    {
        return await _db.KeyExistsAsync("nonexistent:key:12345");
    }
    
    [Benchmark(Description = "DELETE (single)")]
    public async Task<bool> Delete_Single()
    {
        var key = $"del:{Guid.NewGuid()}";
        await _db.StringSetAsync(key, _testValue);
        return await _db.KeyDeleteAsync(key);
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Search Operations (Redis SCAN - note: different semantics)
    // ═══════════════════════════════════════════════════════════════
    
    [Benchmark(Description = "SCAN (pattern match)")]
    public async Task<List<RedisKey>> Scan_Pattern()
    {
        var prefix = _searchKeys[0][..^5]; // Remove the ":0000" suffix
        var server = _redis.GetServer(_redis.GetEndPoints()[0]);
        var keys = new List<RedisKey>();
        
        await foreach (var key in server.KeysAsync(pattern: $"{prefix}*", pageSize: 50))
        {
            keys.Add(key);
            if (keys.Count >= 50) break;
        }
        
        return keys;
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Concurrent Operations
    // ═══════════════════════════════════════════════════════════════
    
    [Benchmark(Description = "SET (100 concurrent)")]
    public async Task Set_Concurrent_100()
    {
        var tasks = _batchKeys.Select((key, i) => 
            _db.StringSetAsync(key, _testValue));
        await Task.WhenAll(tasks);
    }
    
    [Benchmark(Description = "GET (100 concurrent)")]
    public async Task Get_Concurrent_100()
    {
        // First ensure keys exist
        var setTasks = _batchKeys.Select(key => _db.StringSetAsync(key, _testValue));
        await Task.WhenAll(setTasks);
        
        // Then benchmark GET
        var getTasks = _batchKeys.Select(key => _db.StringGetAsync(key));
        await Task.WhenAll(getTasks);
    }
    
    [Benchmark(Description = "Mixed SET/GET (100 concurrent)")]
    public async Task Mixed_Concurrent_100()
    {
        var tasks = new List<Task>();
        for (int i = 0; i < _batchKeys.Length; i++)
        {
            if (i % 2 == 0)
                tasks.Add(_db.StringSetAsync(_batchKeys[i], _testValue));
            else
                tasks.Add(_db.StringGetAsync(_batchKeys[i]));
        }
        await Task.WhenAll(tasks);
    }
}
