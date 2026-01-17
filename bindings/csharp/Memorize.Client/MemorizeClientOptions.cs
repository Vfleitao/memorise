namespace Memorize.Client;

/// <summary>
/// Configuration options for the Memorize client.
/// </summary>
public sealed class MemorizeClientOptions
{
    /// <summary>
    /// The server address. Default is "http://localhost:50051".
    /// </summary>
    public string ServerAddress { get; set; } = "http://localhost:50051";

    /// <summary>
    /// Optional API key for authentication. If null, no authentication is used.
    /// </summary>
    public string? ApiKey { get; set; }

    /// <summary>
    /// Default TTL in seconds for SET operations when not specified. Default is 300 (5 minutes).
    /// </summary>
    public ulong DefaultTtlSeconds { get; set; } = 300;

    /// <summary>
    /// Connection timeout. Default is 5 seconds.
    /// </summary>
    public TimeSpan ConnectTimeout { get; set; } = TimeSpan.FromSeconds(5);

    /// <summary>
    /// Creates options from environment variables.
    /// MEMORIZE_SERVER_URL - Server address
    /// MEMORIZE_API_KEY - API key (optional)
    /// </summary>
    public static MemorizeClientOptions FromEnvironment()
    {
        return new MemorizeClientOptions
        {
            ServerAddress = Environment.GetEnvironmentVariable("MEMORIZE_SERVER_URL") ?? "http://localhost:50051",
            ApiKey = Environment.GetEnvironmentVariable("MEMORIZE_API_KEY")
        };
    }
}
