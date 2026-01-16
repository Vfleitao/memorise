using System.Text.Json;

namespace Memorize.Client;

/// <summary>
/// Extension methods for working with JSON objects in the cache.
/// </summary>
public static class MemorizeClientExtensions
{
    private static readonly JsonSerializerOptions DefaultJsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        WriteIndented = false
    };

    /// <summary>
    /// Gets and deserializes a JSON value from the cache.
    /// </summary>
    /// <typeparam name="T">The type to deserialize to</typeparam>
    /// <param name="client">The Memorize client</param>
    /// <param name="key">The key to look up</param>
    /// <param name="options">JSON serializer options (optional)</param>
    /// <param name="cancellationToken">Cancellation token</param>
    /// <returns>The deserialized object, or default(T) if not found</returns>
    public static async Task<T?> GetJsonAsync<T>(
        this MemorizeClient client,
        string key,
        JsonSerializerOptions? options = null,
        CancellationToken cancellationToken = default)
    {
        var json = await client.GetAsync(key, cancellationToken);
        if (json == null) return default;

        return JsonSerializer.Deserialize<T>(json, options ?? DefaultJsonOptions);
    }

    /// <summary>
    /// Serializes and stores a value as JSON in the cache.
    /// </summary>
    /// <typeparam name="T">The type to serialize</typeparam>
    /// <param name="client">The Memorize client</param>
    /// <param name="key">The key</param>
    /// <param name="value">The value to serialize and store</param>
    /// <param name="ttlSeconds">Time-to-live in seconds</param>
    /// <param name="options">JSON serializer options (optional)</param>
    /// <param name="cancellationToken">Cancellation token</param>
    public static async Task SetJsonAsync<T>(
        this MemorizeClient client,
        string key,
        T value,
        ulong ttlSeconds = 300,
        JsonSerializerOptions? options = null,
        CancellationToken cancellationToken = default)
    {
        var json = JsonSerializer.Serialize(value, options ?? DefaultJsonOptions);
        await client.SetAsync(key, json, ttlSeconds, cancellationToken);
    }

    /// <summary>
    /// Serializes and stores a value as JSON in the cache.
    /// </summary>
    /// <typeparam name="T">The type to serialize</typeparam>
    /// <param name="client">The Memorize client</param>
    /// <param name="key">The key</param>
    /// <param name="value">The value to serialize and store</param>
    /// <param name="ttl">Time-to-live duration</param>
    /// <param name="options">JSON serializer options (optional)</param>
    /// <param name="cancellationToken">Cancellation token</param>
    public static Task SetJsonAsync<T>(
        this MemorizeClient client,
        string key,
        T value,
        TimeSpan ttl,
        JsonSerializerOptions? options = null,
        CancellationToken cancellationToken = default)
    {
        return SetJsonAsync(client, key, value, (ulong)ttl.TotalSeconds, options, cancellationToken);
    }

    /// <summary>
    /// Gets or sets a JSON value using the cache-aside pattern.
    /// </summary>
    /// <typeparam name="T">The type to cache</typeparam>
    /// <param name="client">The Memorize client</param>
    /// <param name="key">The key</param>
    /// <param name="factory">Factory to create the value if not cached</param>
    /// <param name="ttlSeconds">TTL for newly created values</param>
    /// <param name="options">JSON serializer options (optional)</param>
    /// <param name="cancellationToken">Cancellation token</param>
    /// <returns>The cached or newly created value</returns>
    public static async Task<T> GetOrSetJsonAsync<T>(
        this MemorizeClient client,
        string key,
        Func<Task<T>> factory,
        ulong ttlSeconds = 300,
        JsonSerializerOptions? options = null,
        CancellationToken cancellationToken = default)
    {
        var existing = await client.GetJsonAsync<T>(key, options, cancellationToken);
        if (existing != null)
            return existing;

        var value = await factory();
        await client.SetJsonAsync(key, value, ttlSeconds, options, cancellationToken);
        return value;
    }

    /// <summary>
    /// Gets or sets a JSON value using the cache-aside pattern.
    /// </summary>
    /// <typeparam name="T">The type to cache</typeparam>
    /// <param name="client">The Memorize client</param>
    /// <param name="key">The key</param>
    /// <param name="factory">Factory to create the value if not cached</param>
    /// <param name="ttl">TTL for newly created values</param>
    /// <param name="options">JSON serializer options (optional)</param>
    /// <param name="cancellationToken">Cancellation token</param>
    /// <returns>The cached or newly created value</returns>
    public static Task<T> GetOrSetJsonAsync<T>(
        this MemorizeClient client,
        string key,
        Func<Task<T>> factory,
        TimeSpan ttl,
        JsonSerializerOptions? options = null,
        CancellationToken cancellationToken = default)
    {
        return GetOrSetJsonAsync(client, key, factory, (ulong)ttl.TotalSeconds, options, cancellationToken);
    }
}
