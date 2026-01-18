# Memorize - Build Output

Built: 2026-01-18 16:54:59
Configuration: Release

## Contents

| File | Description |
|------|-------------|
| `bin/memorize-server.exe` | The gRPC cache server |
| `bin/memorize-integration-tests.exe` | Rust integration tests |
| `bin/memorize-integration-tests-csharp.exe` | C# integration tests |
| `proto/memorize.proto` | Protocol Buffer definition |
| `nuget/Memorize.Client.*.nupkg` | C# client library NuGet package |

## Quick Start

```powershell
# Start the server
./run-server.ps1

# With authentication
./run-server.ps1 -ApiKey "your-secret-key"

# In another terminal, run tests
./run-tests.ps1
./run-tests.ps1 -ApiKey "your-secret-key"
```

## Using the C# Client Library

```powershell
# Install from local NuGet
dotnet add package Memorize.Client --source ./nuget
```

```csharp
using Memorize.Client;

// Basic usage
using var cache = new MemorizeClient("http://localhost:50051");
await cache.SetAsync("key", "value", ttlSeconds: 300);
var value = await cache.GetAsync("key");

// With authentication
var options = new MemorizeClientOptions 
{ 
    ServerUrl = "http://localhost:50051",
    ApiKey = "your-secret-key"
};
using var authCache = new MemorizeClient(options);

// JSON serialization (extension methods)
await cache.SetJsonAsync("user:1", new { Name = "John", Age = 30 });
var user = await cache.GetJsonAsync<User>("user:1");

// Cache-aside pattern
var data = await cache.GetOrSetAsync("expensive-key", async () => {
    return await LoadFromDatabaseAsync();
}, ttlSeconds: 600);
```

## Server Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `MEMORIZE_HOST` | 127.0.0.1 | Bind address |
| `MEMORIZE_PORT` | 50051 | Port number |
| `MEMORIZE_CLEANUP_INTERVAL` | 60 | Cleanup interval (seconds) |
| `MEMORIZE_API_KEY` | (none) | API key (if unset, auth disabled) |
