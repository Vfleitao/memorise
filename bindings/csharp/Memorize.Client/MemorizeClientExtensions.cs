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
}
