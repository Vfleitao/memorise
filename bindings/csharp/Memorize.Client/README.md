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

## Cache-Aside Pattern

Automatically fetch and cache data:

```csharp
// If "user:1" exists, return it
// Otherwise, call the factory, cache the result, and return it
var user = await cache.GetOrSetJsonAsync(
    "user:1",
    async () => await _database.GetUserAsync(1),  // Only called on cache miss
    ttlSeconds: 600
);
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
        return await _cache.GetOrSetJsonAsync(
            $"user:{id}",
            () => _db.Users.FindAsync(id),
            TimeSpan.FromMinutes(5)
        );
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

## License

MIT
