# Memorize

A Redis-like in-memory cache server with gRPC API, written in Rust.

## Features

- **Fast**: Built with Rust and DashMap for lock-free concurrent access
- **gRPC API**: Language-agnostic protocol with official C# and Rust client bindings
- **TTL Support**: Automatic key expiration with background cleanup
- **API Key Authentication**: Optional security via `x-api-key` header
- **Docker Ready**: Multi-stage build for minimal ~25MB container image
- **Cross-Platform**: Runs on Windows, Linux, and macOS

> [!IMPORTANT]
> Memorize is a **simple, single-node cache** designed for straightforward read/write operations. It does not support clustering, replication, or persistence. If you need those features, consider Redis or similar solutions.

## Performance

Benchmarks comparing Memorize vs Redis (StackExchange.Redis) on Windows 11, .NET 8.0:

### Single Operations (100-byte payload)

| Operation | Memorize | Redis | Notes |
|-----------|----------|-------|-------|
| **SET** | 234 μs | 189 μs | gRPC has ~20% higher protocol overhead |
| **GET** | 231 μs | 187 μs | Similar pattern for reads |
| **CONTAINS/EXISTS** | 231-237 μs | 182-201 μs | Key existence checks |
| **DELETE** | 479 μs | 402 μs | Includes roundtrip confirmation |
| **SET+GET roundtrip** | 744 μs | 601 μs | Full write-read cycle |

### Key Search Operations

| Operation | Memorize | Redis | Notes |
|-----------|----------|-------|-------|
| **SEARCH_KEYS (50 results)** | 313 μs | 225 μs | Pattern matching with results |
| **SEARCH_KEYS (paginated 10)** | 252 μs | N/A | Memorize supports native pagination |

### Memory Allocation

| Operation | Memorize | Redis |
|-----------|----------|-------|
| **SET (single)** | 6.6 KB | 0.4 KB |
| **GET (single)** | 6.8 KB | 0.6 KB |
| **SET+GET roundtrip** | 19.8 KB | 1.2 KB |

> [!NOTE]
> **Trade-offs**: Memorize uses gRPC which provides language-agnostic, strongly-typed APIs with automatic code generation, but has higher per-request overhead than Redis's optimized binary protocol. Memorize excels when you need a simple, self-contained cache without external dependencies, built-in pagination for key searches, or prefer gRPC for service mesh integration.

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

The official image is available on GitHub Container Registry:

```bash
docker pull ghcr.io/vfleitao/memorise:latest
```

```bash
# Run with default settings (no authentication, 100MB limit)
docker run -d -p 50051:50051 --name memorize ghcr.io/vfleitao/memorise:latest

# Run with API key authentication
docker run -d -p 50051:50051 -e MEMORIZE_API_KEY=your-secret-key ghcr.io/vfleitao/memorise:latest

# Run with custom storage limit (500MB)
docker run -d -p 50051:50051 -e MEMORIZE_MAX_STORAGE_MB=500 ghcr.io/vfleitao/memorise:latest

# Run with all custom settings
docker run -d -p 50051:50051 \
  -e MEMORIZE_HOST=0.0.0.0 \
  -e MEMORIZE_PORT=50051 \
  -e MEMORIZE_CLEANUP_INTERVAL=30 \
  -e MEMORIZE_MAX_STORAGE_MB=500 \
  -e MEMORIZE_API_KEY=my-secret \
  ghcr.io/vfleitao/memorise:latest
```

### Deploying to Azure

> [!WARNING]
> **Security Notice**: Memorize does not provide TLS/SSL encryption. It is designed to run **inside a private network (VNet)** where only your application can access it. For production deployments:
> - Deploy within a Virtual Network (VNet) with no public IP
> - Use Azure Private Endpoints or internal load balancers
> - If public access is required, place behind an API Gateway or reverse proxy that handles TLS termination
> - Always enable API key authentication (`MEMORIZE_API_KEY`)

#### Azure Container Instances (with VNet)

```bash
# Create a resource group
az group create --name memorize-rg --location eastus

# Create a VNet and subnet for the container
az network vnet create \
  --resource-group memorize-rg \
  --name memorize-vnet \
  --address-prefix 10.0.0.0/16 \
  --subnet-name memorize-subnet \
  --subnet-prefix 10.0.0.0/24

# Deploy the container (private IP only - no public access)
az container create \
  --resource-group memorize-rg \
  --name memorize-cache \
  --image ghcr.io/vfleitao/memorise:latest \
  --ports 50051 \
  --cpu 1 \
  --memory 1 \
  --environment-variables \
    MEMORIZE_MAX_STORAGE_MB=500 \
    MEMORIZE_API_KEY=your-secret-key \
  --vnet memorize-vnet \
  --subnet memorize-subnet \
  --ip-address private

# Get the private IP address (use this from your app in the same VNet)
az container show \
  --resource-group memorize-rg \
  --name memorize-cache \
  --query ipAddress.ip \
  --output tsv
```

#### Docker Compose (Local Development)

```yaml
# docker-compose.yml
services:
  memorize:
    image: ghcr.io/vfleitao/memorise:latest
    ports:
      - "50051:50051"
    environment:
      - MEMORIZE_MAX_STORAGE_MB=500
      - MEMORIZE_API_KEY=dev-secret-key
    restart: unless-stopped
```

```bash
docker-compose up -d
```

#### Docker Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MEMORIZE_HOST` | `0.0.0.0` | Host address to bind |
| `MEMORIZE_PORT` | `50051` | gRPC port |
| `MEMORIZE_CLEANUP_INTERVAL` | `60` | Seconds between TTL cleanup runs |
| `MEMORIZE_MAX_STORAGE_MB` | `100` | Maximum storage size in MB (0 = unlimited) |
| `MEMORIZE_API_KEY` | *(none)* | API key for authentication (disabled if not set) |

### Building from Source

```powershell
# Clone the repository
git clone https://github.com/Vfleitao/memorise.git
cd memorise

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

# With custom storage limit (500MB)
./dist/run-server.ps1 -MaxStorageMB 500

# With unlimited storage (use with caution!)
./dist/run-server.ps1 -MaxStorageMB 0

# With authentication
./dist/run-server.ps1 -ApiKey "my-secret"

# Or run directly with environment variables
$env:MEMORIZE_MAX_STORAGE_MB = 500
$env:MEMORIZE_API_KEY = "my-secret"
./dist/bin/memorize-server.exe
```

## Client Usage

### C# (.NET)

Install from [NuGet](https://www.nuget.org/packages/Memorize.Client):

```bash
dotnet add package Memorize.Client
```

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

### Unit Tests

```powershell
# Build and run all unit tests
./build.ps1
```

### Integration Tests

Integration tests require a running server:

```powershell
# Terminal 1: Start the server
./dist/run-server.ps1

# Terminal 2: Run all integration tests (Rust + C#)
./dist/run-tests.ps1

# Or run with authentication
./dist/run-server.ps1 -ApiKey "test-key"
./dist/run-tests.ps1 -ApiKey "test-key"
```

Or run the test binaries directly:

```powershell
./dist/bin/memorize-integration-tests.exe      # Rust tests
./dist/bin/memorize-integration-tests-csharp.exe  # C# tests
```

## License

This project is licensed under the **Memorize Source Available License (MSAL)**.

**In short:**
- ✅ Free to use in your applications (even commercial ones)
- ✅ Free to modify and distribute
- ✅ Improvements must be contributed back to the project
- ✅ Attribution required
- ❌ Cache service businesses (like Azure Redis Cache) need a separate license

See [LICENSE](LICENSE) for full terms.

---

> [!NOTE]
> This project was built using **AI-Assisted Development** — human-driven design, architecture, and code review with AI as a productivity tool. Not vibe-coding.

