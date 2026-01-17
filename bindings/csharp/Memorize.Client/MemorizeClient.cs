using Grpc.Core;
using Grpc.Core.Interceptors;
using Grpc.Net.Client;
using Proto = global::Memorize;

namespace Memorize.Client;

/// <summary>
/// A simple, high-performance client for the Memorize cache server.
/// Thread-safe and designed for long-lived use.
/// </summary>
/// <example>
/// <code>
/// // Simple usage
/// using var cache = new MemorizeClient("http://localhost:50051");
/// await cache.SetAsync("user:123", "{\"name\":\"John\"}", ttlSeconds: 600);
/// var user = await cache.GetAsync("user:123");
/// 
/// // With authentication
/// var options = new MemorizeClientOptions 
/// {
///     ServerAddress = "http://localhost:50051",
///     ApiKey = "my-secret-key"
/// };
/// using var cache = new MemorizeClient(options);
/// </code>
/// </example>
public sealed class MemorizeClient : IDisposable, IAsyncDisposable
{
    private readonly GrpcChannel _channel;
    private readonly Proto.Memorize.MemorizeClient _grpcClient;
    private readonly MemorizeClientOptions _options;
    private bool _disposed;

    /// <summary>
    /// Creates a new Memorize client with default options (localhost:50051, no auth).
    /// </summary>
    public MemorizeClient() : this(new MemorizeClientOptions())
    {
    }

    /// <summary>
    /// Creates a new Memorize client connecting to the specified server.
    /// </summary>
    /// <param name="serverAddress">The server address (e.g., "http://localhost:50051")</param>
    public MemorizeClient(string serverAddress) : this(new MemorizeClientOptions { ServerAddress = serverAddress })
    {
    }

    /// <summary>
    /// Creates a new Memorize client connecting to the specified server with an API key.
    /// </summary>
    /// <param name="serverAddress">The server address (e.g., "http://localhost:50051")</param>
    /// <param name="apiKey">The API key for authentication</param>
    public MemorizeClient(string serverAddress, string apiKey) 
        : this(new MemorizeClientOptions { ServerAddress = serverAddress, ApiKey = apiKey })
    {
    }

    /// <summary>
    /// Creates a new Memorize client with the specified options.
    /// </summary>
    /// <param name="options">Client configuration options</param>
    public MemorizeClient(MemorizeClientOptions options)
    {
        _options = options ?? throw new ArgumentNullException(nameof(options));
        
        _channel = GrpcChannel.ForAddress(options.ServerAddress, new GrpcChannelOptions
        {
            HttpHandler = new SocketsHttpHandler
            {
                ConnectTimeout = options.ConnectTimeout,
                EnableMultipleHttp2Connections = true
            }
        });

        if (!string.IsNullOrEmpty(options.ApiKey))
        {
            var invoker = _channel.Intercept(new ApiKeyInterceptor(options.ApiKey));
            _grpcClient = new Proto.Memorize.MemorizeClient(invoker);
        }
        else
        {
            _grpcClient = new Proto.Memorize.MemorizeClient(_channel);
        }
    }

    /// <summary>
    /// Gets a value from the cache.
    /// </summary>
    /// <param name="key">The key to look up</param>
    /// <param name="cancellationToken">Cancellation token</param>
    /// <returns>The value if found, null if not found or expired</returns>
    public async Task<string?> GetAsync(string key, CancellationToken cancellationToken = default)
    {
        ThrowIfDisposed();
        ArgumentException.ThrowIfNullOrEmpty(key);

        var response = await _grpcClient.GetAsync(
            new Proto.GetRequest { Key = key }, 
            cancellationToken: cancellationToken);

        return response.HasValue ? response.Value : null;
    }

    /// <summary>
    /// Gets a value from the cache, returning a default value if not found.
    /// </summary>
    /// <param name="key">The key to look up</param>
    /// <param name="defaultValue">Value to return if key not found</param>
    /// <param name="cancellationToken">Cancellation token</param>
    /// <returns>The cached value or the default value</returns>
    public async Task<string> GetOrDefaultAsync(string key, string defaultValue, CancellationToken cancellationToken = default)
    {
        return await GetAsync(key, cancellationToken) ?? defaultValue;
    }

    /// <summary>
    /// Sets a value in the cache with the default TTL.
    /// </summary>
    /// <param name="key">The key</param>
    /// <param name="value">The value to store</param>
    /// <param name="cancellationToken">Cancellation token</param>
    public Task SetAsync(string key, string value, CancellationToken cancellationToken = default)
    {
        return SetAsync(key, value, _options.DefaultTtlSeconds, cancellationToken);
    }

    /// <summary>
    /// Sets a value in the cache with a specific TTL.
    /// </summary>
    /// <param name="key">The key</param>
    /// <param name="value">The value to store</param>
    /// <param name="ttlSeconds">Time-to-live in seconds (0 = never expire)</param>
    /// <param name="cancellationToken">Cancellation token</param>
    public async Task SetAsync(string key, string value, ulong ttlSeconds, CancellationToken cancellationToken = default)
    {
        ThrowIfDisposed();
        ArgumentException.ThrowIfNullOrEmpty(key);
        ArgumentNullException.ThrowIfNull(value);

        await _grpcClient.SetAsync(
            new Proto.SetRequest { Key = key, Value = value, TtlSeconds = ttlSeconds },
            cancellationToken: cancellationToken);
    }

    /// <summary>
    /// Sets a value in the cache that never expires.
    /// </summary>
    /// <param name="key">The key</param>
    /// <param name="value">The value to store</param>
    /// <param name="cancellationToken">Cancellation token</param>
    public Task SetWithoutExpirationAsync(string key, string value, CancellationToken cancellationToken = default)
    {
        return SetAsync(key, value, 0, cancellationToken);
    }

    /// <summary>
    /// Sets a value in the cache with a specific TTL.
    /// </summary>
    /// <param name="key">The key</param>
    /// <param name="value">The value to store</param>
    /// <param name="ttl">Time-to-live duration</param>
    /// <param name="cancellationToken">Cancellation token</param>
    public Task SetAsync(string key, string value, TimeSpan ttl, CancellationToken cancellationToken = default)
    {
        return SetAsync(key, value, (ulong)ttl.TotalSeconds, cancellationToken);
    }

    /// <summary>
    /// Deletes a key from the cache.
    /// </summary>
    /// <param name="key">The key to delete</param>
    /// <param name="cancellationToken">Cancellation token</param>
    /// <returns>True if the key existed and was deleted, false otherwise</returns>
    public async Task<bool> DeleteAsync(string key, CancellationToken cancellationToken = default)
    {
        ThrowIfDisposed();
        ArgumentException.ThrowIfNullOrEmpty(key);

        var response = await _grpcClient.DeleteAsync(
            new Proto.DeleteRequest { Key = key },
            cancellationToken: cancellationToken);

        return response.Deleted;
    }

    /// <summary>
    /// Checks if a key exists in the cache (and is not expired).
    /// </summary>
    /// <param name="key">The key to check</param>
    /// <param name="cancellationToken">Cancellation token</param>
    /// <returns>True if the key exists, false otherwise</returns>
    public async Task<bool> ContainsAsync(string key, CancellationToken cancellationToken = default)
    {
        ThrowIfDisposed();
        ArgumentException.ThrowIfNullOrEmpty(key);

        var response = await _grpcClient.ContainsAsync(
            new Proto.ContainsRequest { Key = key },
            cancellationToken: cancellationToken);

        return response.Exists;
    }

    /// <summary>
    /// Gets all keys in the cache.
    /// </summary>
    /// <param name="limit">Maximum number of keys to return (0 = server default of 10000)</param>
    /// <param name="cancellationToken">Cancellation token</param>
    /// <returns>A list of all keys</returns>
    public async Task<IReadOnlyList<string>> GetKeysAsync(uint limit = 0, CancellationToken cancellationToken = default)
    {
        ThrowIfDisposed();

        var response = await _grpcClient.KeysAsync(
            new Proto.KeysRequest { Limit = limit },
            cancellationToken: cancellationToken);

        return response.Keys.ToList();
    }

    private void ThrowIfDisposed()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
    }

    /// <inheritdoc />
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _channel.Dispose();
    }

    /// <inheritdoc />
    public async ValueTask DisposeAsync()
    {
        if (_disposed) return;
        _disposed = true;
        await _channel.ShutdownAsync();
        _channel.Dispose();
    }
}

/// <summary>
/// Internal interceptor for API key authentication
/// </summary>
internal sealed class ApiKeyInterceptor : Interceptor
{
    private const string ApiKeyHeader = "x-api-key";
    private readonly string _apiKey;

    public ApiKeyInterceptor(string apiKey)
    {
        _apiKey = apiKey;
    }

    public override AsyncUnaryCall<TResponse> AsyncUnaryCall<TRequest, TResponse>(
        TRequest request,
        ClientInterceptorContext<TRequest, TResponse> context,
        AsyncUnaryCallContinuation<TRequest, TResponse> continuation)
    {
        var headers = context.Options.Headers ?? new Metadata();
        headers.Add(ApiKeyHeader, _apiKey);

        var newOptions = context.Options.WithHeaders(headers);
        var newContext = new ClientInterceptorContext<TRequest, TResponse>(
            context.Method, context.Host, newOptions);

        return continuation(request, newContext);
    }
}
