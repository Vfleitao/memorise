# Memorize.Client

A simple, high-performance .NET client for the Memorize cache server.

## Installation

```bash
# From NuGet (when published)
dotnet add package Memorize.Client

# Or add project reference
dotnet add reference path/to/Memorize.Client.csproj
```

## Quick Start

```csharp
using Memorize.Client;

// Create a client
using var cache = new MemorizeClient("http://localhost:50051");

// Set a value (default 5 minute TTL)
await cache.SetAsync("user:123", "John Doe");

// Set with custom TTL
await cache.SetAsync("session:abc", "token123", ttlSeconds: 3600);
await cache.SetAsync("temp", "data", TimeSpan.FromMinutes(10));

// Get a value
string? value = await cache.GetAsync("user:123");

// Get with default
string value = await cache.GetOrDefaultAsync("missing", "default");

// Check existence
bool exists = await cache.ContainsAsync("user:123");

// Delete
bool deleted = await cache.DeleteAsync("user:123");

// List all keys
IReadOnlyList<string> keys = await cache.GetKeysAsync();
```

## Configuration

```csharp
// Using options
var options = new MemorizeClientOptions
{
    ServerAddress = "http://localhost:50051",
    ApiKey = "my-secret-key",           // Optional authentication
    DefaultTtlSeconds = 600,            // Default TTL for Set operations
    ConnectTimeout = TimeSpan.FromSeconds(5)
};
using var cache = new MemorizeClient(options);

// From environment variables
// Set: MEMORIZE_SERVER_URL, MEMORIZE_API_KEY
using var cache = new MemorizeClient(MemorizeClientOptions.FromEnvironment());
```

## JSON Serialization

Extension methods for automatic JSON serialization:

```csharp
using Memorize.Client;

// Define your types
record User(string Name, string Email, int Age);

// Store as JSON
var user = new User("Alice", "alice@example.com", 30);
await cache.SetJsonAsync("user:1", user, ttlSeconds: 300);

// Retrieve and deserialize
User? loaded = await cache.GetJsonAsync<User>("user:1");
```

## Thread Safety

`MemorizeClient` is thread-safe and designed for long-lived use. Create one instance and share it across your application:

```csharp
// In Startup.cs / Program.cs
builder.Services.AddSingleton<MemorizeClient>(sp => 
    new MemorizeClient(MemorizeClientOptions.FromEnvironment()));

// In your service
public class UserService
{
    private readonly MemorizeClient _cache;
    
    public UserService(MemorizeClient cache) => _cache = cache;
    
    public async Task<User?> GetUserAsync(int id)
    {
        // Try cache first
        var cached = await _cache.GetJsonAsync<User>($"user:{id}");
        if (cached != null)
            return cached;

        // Cache miss - fetch from database
        var user = await _db.Users.FindAsync(id);
        if (user != null)
            await _cache.SetJsonAsync($"user:{id}", user, TimeSpan.FromMinutes(5));

        return user;
    }
}
```

## ASP.NET Core Integration

```csharp
// Program.cs
var builder = WebApplication.CreateBuilder(args);

// Register as singleton
builder.Services.AddSingleton<MemorizeClient>(sp =>
{
    var config = sp.GetRequiredService<IConfiguration>();
    return new MemorizeClient(new MemorizeClientOptions
    {
        ServerAddress = config["Memorize:ServerAddress"] ?? "http://localhost:50051",
        ApiKey = config["Memorize:ApiKey"]
    });
});

var app = builder.Build();

// Use in endpoints
app.MapGet("/users/{id}", async (int id, MemorizeClient cache) =>
{
    var user = await cache.GetJsonAsync<User>($"user:{id}");
    return user is not null ? Results.Ok(user) : Results.NotFound();
});
```

## Fire and Forget

For non-critical cache updates, you can fire and forget:

```csharp
// Don't await - fire and forget
_ = cache.SetAsync("analytics:pageview", DateTime.UtcNow.ToString());

// Continue immediately
return Results.Ok();
```

## Error Handling

```csharp
try
{
    await cache.SetAsync("key", "value");
}
catch (StorageFullException ex)
{
    // Server has reached its maximum storage limit
    // Consider evicting old entries or alerting ops
    _logger.LogError("Cache storage full: {Message}", ex.Message);
}
catch (RpcException ex) when (ex.StatusCode == StatusCode.Unavailable)
{
    // Server not available - handle gracefully
    _logger.LogWarning("Cache unavailable, continuing without cache");
}
catch (RpcException ex) when (ex.StatusCode == StatusCode.Unauthenticated)
{
    // Invalid API key
    throw new InvalidOperationException("Invalid cache API key", ex);
}
```

### Storage Full Exception

When the server has reached its configured maximum storage limit (`MEMORIZE_MAX_STORAGE_MB`), `SetAsync` will throw a `StorageFullException`:

```csharp
try
{
    await cache.SetAsync("key", largeValue);
}
catch (StorageFullException)
{
    // Option 1: Try to free space by deleting old entries
    await cache.DeleteAsync("old-key");
    await cache.SetAsync("key", largeValue);
    
    // Option 2: Use a smaller TTL to let entries expire faster
    await cache.SetAsync("key", value, ttlSeconds: 60);
    
    // Option 3: Log and continue without caching
    _logger.LogWarning("Cache full, skipping cache write");
}

// Extension method for checking
catch (Exception ex) when (ex.IsStorageFull())
{
    // Handle storage full
}
```

## License

MIT
