# Memorize

A high-performance, Redis-like in-memory cache server with gRPC API, written in Rust.

## Features

- **Fast**: Built with Rust and DashMap for lock-free concurrent access
- **gRPC API**: Language-agnostic protocol with official C# and Rust client bindings
- **TTL Support**: Automatic key expiration with background cleanup
- **API Key Authentication**: Optional security via `x-api-key` header
- **Docker Ready**: Multi-stage build for minimal ~25MB container image
- **Cross-Platform**: Runs on Windows, Linux, and macOS

## Architecture

```
memorize/
├── memorize-core/      # Core storage engine (DashMap + TTL)
├── memorize-proto/     # Protocol Buffers definitions
├── memorize-server/    # gRPC server implementation
└── bindings/
    ├── csharp/         # C# client library (NuGet)
    └── rust/           # Rust client library (Cargo)
```

## Prerequisites

### For Building from Source

- **Rust** 1.75+ (`rustup` recommended)
- **Protocol Buffers Compiler** (`protoc`)
  - Windows: `winget install Google.Protobuf`
  - macOS: `brew install protobuf`
  - Linux: `apt install protobuf-compiler`
- **PowerShell 7+** (for build script)
- **.NET 8 SDK** (for C# bindings only)

### For Docker Only

- **Docker** 20.10+

## Quick Start

### Using Docker (Recommended)

```bash
# Build the image
docker build -t memorize-server .

# Run with default settings (no authentication)
docker run -d -p 50051:50051 --name memorize memorize-server

# Run with API key authentication
docker run -d -p 50051:50051 -e MEMORIZE_API_KEY=your-secret-key memorize-server

# Run with custom settings
docker run -d -p 50051:50051 \
  -e MEMORIZE_HOST=0.0.0.0 \
  -e MEMORIZE_PORT=50051 \
  -e MEMORIZE_CLEANUP_INTERVAL=30 \
  -e MEMORIZE_API_KEY=my-secret \
  memorize-server
```

#### Docker Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MEMORIZE_HOST` | `0.0.0.0` | Host address to bind |
| `MEMORIZE_PORT` | `50051` | gRPC port |
| `MEMORIZE_CLEANUP_INTERVAL` | `60` | Seconds between TTL cleanup runs |
| `MEMORIZE_API_KEY` | *(none)* | API key for authentication (disabled if not set) |

### Building from Source

```powershell
# Clone the repository
git clone https://github.com/your-username/memorize.git
cd memorize

# Build everything (server, tests, bindings, NuGet package)
./build.ps1

# Build without running tests
./build.ps1 -SkipTests

# Build in debug mode
./build.ps1 -Configuration Debug

# Build to custom output directory
./build.ps1 -OutputDir ./my-artifacts
```

After building, artifacts are in `./dist/`:

```
dist/
├── bin/
│   ├── memorize-server.exe         # The server binary
│   ├── memorize-integration-tests.exe    # Rust integration tests
│   └── memorize-integration-tests-csharp.exe  # C# integration tests
├── proto/
│   └── memorize.proto              # Protocol definition
├── nuget/
│   └── Memorize.Client.1.0.0.nupkg # C# NuGet package
├── run-server.ps1                  # Helper script to run server
└── run-tests.ps1                   # Helper script to run tests
```

### Running the Server

```powershell
# Using the helper script
./dist/run-server.ps1

# Or directly
./dist/bin/memorize-server.exe

# With environment variables
$env:MEMORIZE_API_KEY = "my-secret"
./dist/bin/memorize-server.exe
```

## Client Usage

### C# (.NET)

Install the NuGet package or reference the built `.nupkg`:

```csharp
using Memorize.Client;

// Simple connection
using var cache = new MemorizeClient("http://localhost:50051");

// With authentication
using var cache = new MemorizeClient("http://localhost:50051", "your-api-key");

// Basic operations
await cache.SetAsync("user:123", "{\"name\":\"Alice\"}", ttlSeconds: 600);
var value = await cache.GetAsync("user:123");
var exists = await cache.ContainsAsync("user:123");
var deleted = await cache.DeleteAsync("user:123");
var keys = await cache.GetKeysAsync();

// JSON helpers (automatic serialization)
await cache.SetJsonAsync("user:123", new User { Name = "Alice" }, TimeSpan.FromMinutes(10));
var user = await cache.GetJsonAsync<User>("user:123");
```

### Rust

Add to your `Cargo.toml`:

```toml
[dependencies]
memorize-client = { path = "../bindings/rust/memorize-client" }
# Or when published: memorize-client = "1.0"
```

```rust
use memorize_client::{MemorizeClient, MemorizeClientOptions};

#[tokio::main]
async fn main() -> Result<(), memorize_client::Error> {
    // Simple connection
    let client = MemorizeClient::connect("http://localhost:50051").await?;

    // With authentication
    let options = MemorizeClientOptions::new("http://localhost:50051")
        .with_api_key("your-api-key");
    let client = MemorizeClient::with_options(options).await?;

    // Basic operations
    client.set("key", "value", Some(300)).await?;  // 5-minute TTL
    let value = client.get("key").await?;
    let exists = client.contains("key").await?;
    let deleted = client.delete("key").await?;
    let keys = client.keys(None).await?;

    // JSON helpers (enable "json" feature)
    client.set_json("user:1", &user, Some(600)).await?;
    let user: Option<User> = client.get_json("user:1").await?;

    Ok(())
}
```

## gRPC API

The server exposes five operations:

| Method | Description |
|--------|-------------|
| `Set(key, value, ttl_seconds)` | Store a value with optional TTL |
| `Get(key)` | Retrieve a value |
| `Delete(key)` | Remove a key |
| `Contains(key)` | Check if key exists |
| `Keys(pattern)` | List keys (optional glob pattern) |

See [memorize-proto/proto/memorize.proto](memorize-proto/proto/memorize.proto) for the full protocol definition.

## Running Tests

```powershell
# Run all tests (unit + integration)
./build.ps1

# Run integration tests against a running server
./dist/run-tests.ps1

# Or manually:
# Terminal 1: Start server
./dist/bin/memorize-server.exe

# Terminal 2: Run tests
./dist/bin/memorize-integration-tests.exe      # Rust
./dist/bin/memorize-integration-tests-csharp.exe  # C#
```

## License

MIT

