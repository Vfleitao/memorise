# Memorize Benchmarks

Performance benchmarks comparing Memorize and Redis using [BenchmarkDotNet](https://benchmarkdotnet.org/).

## Prerequisites

- .NET 8.0 SDK
- Memorize server running locally (or specify `MEMORIZE_URL`)
- Redis server running locally (or specify `REDIS_HOST`) for comparison benchmarks

## Running Benchmarks

### Start the Servers

```powershell
# Start Memorize server
./dist/run-server.ps1

# Start Redis (Docker)
docker run -d --name redis -p 6379:6379 redis:alpine
```

### Run Memorize Benchmarks Only

```powershell
cd bindings/csharp/Memorize.Benchmarks
dotnet run -c Release -- --memorize
```

### Run Redis Benchmarks Only

```powershell
dotnet run -c Release -- --redis
```

### Run Both (Comparison)

```powershell
dotnet run -c Release -- --all
```

### Quick Test (shorter iterations)

```powershell
dotnet run -c Release -- --quick --memorize
dotnet run -c Release -- --quick --redis
dotnet run -c Release -- --quick --all
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MEMORIZE_URL` | `http://localhost:50051` | Memorize server endpoint |
| `MEMORIZE_API_KEY` | *(none)* | Memorize API key if auth enabled |
| `REDIS_HOST` | `localhost:6379` | Redis server endpoint |

## Benchmark Categories

### Single Operations
- **SET** - Write a single key-value pair
- **GET** - Read a single key
- **SET+GET** - Full roundtrip (write, read, delete)
- **CONTAINS/EXISTS** - Check if key exists
- **DELETE** - Remove a key

### Search Operations
- **Memorize**: `SEARCH_KEYS` - Prefix search with pagination
- **Redis**: `SCAN` - Pattern-based key scanning

### Concurrent Operations
- **SET (100 concurrent)** - Parallel writes
- **GET (100 concurrent)** - Parallel reads
- **Mixed (100 concurrent)** - Interleaved read/write operations

## Expected Differences

| Feature | Memorize | Redis |
|---------|----------|-------|
| Protocol | gRPC (HTTP/2) | Redis protocol (TCP) |
| Search | Prefix with pagination | SCAN pattern matching |
| Persistence | None (in-memory only) | Optional (RDB/AOF) |
| Clustering | Not supported | Supported |
| Network overhead | Higher (HTTP/2) | Lower (custom protocol) |

## Benchmark Results

**Test Environment:**
- **CPU**: Intel® Core™ Ultra 9 285HX (24 cores)
- **RAM**: 64GB
- **OS**: Windows 11 with Docker Desktop
- **Both servers running in Docker containers** (fair comparison - same virtualization overhead)

### Protocol Comparison

> **Important**: These benchmarks compare two different protocols:
> - **Redis**: Uses a custom binary protocol over direct **TCP** connection, optimized for minimal latency
> - **Memorize**: Uses **gRPC over HTTP/2**, which has higher overhead but provides cross-language compatibility, built-in streaming, and standard HTTP/2 features

### Single Operations (1KB payload)

| Operation | Memorize (gRPC) | Redis (TCP) | Winner |
|-----------|-----------------|-------------|--------|
| SET | ~66 μs | ~187 μs | **Memorize** ~2.8x faster |
| GET | ~69 μs | ~207 μs | **Memorize** ~3x faster |
| SET+GET (roundtrip) | ~269 μs | ~586 μs | **Memorize** ~2.2x faster |
| EXISTS (hit) | ~67 μs | ~212 μs | **Memorize** ~3.2x faster |
| EXISTS (miss) | ~68 μs | ~196 μs | **Memorize** ~2.9x faster |
| DELETE | ~403 μs | ~381 μs | Similar |
| SEARCH/SCAN | ~198 μs | ~257 μs | **Memorize** ~1.3x faster |

### Concurrent Operations (100 parallel, 1KB payload)

| Operation | Memorize (gRPC) | Redis (TCP) | Winner |
|-----------|-----------------|-------------|--------|
| SET (100 concurrent) | ~1,406 μs | ~391 μs | **Redis** ~3.6x faster |
| GET (100 concurrent) | ~5,550 μs | ~758 μs | **Redis** ~7.3x faster |
| Mixed SET/GET (100 concurrent) | ~1,490 μs | ~393 μs | **Redis** ~3.8x faster |

### Memory Allocation per Operation

| Operation | Memorize | Redis |
|-----------|----------|-------|
| SET (single) | ~6.8 KB | ~432 B |
| GET (single) | ~8.8 KB | ~1.5 KB |
| SET (100 concurrent) | ~698 KB | ~33 KB |
| GET (100 concurrent) | ~858 KB | ~173 KB |

### Key Takeaways

1. **Single operations (common use case)**: **Memorize is 2-3x faster** for simple GET/SET operations
2. **Concurrent workloads**: **Redis is 3-7x faster** under high parallelism due to its multiplexed protocol
3. **Memory**: Redis uses significantly less memory per operation (~15x less for single ops)
4. **Best for**:
   - **Memorize**: Applications with sequential cache access, microservices needing gRPC/HTTP2 compatibility
   - **Redis**: High-throughput systems with concurrent access patterns, memory-constrained environments

> **Note**: Both servers ran in Docker containers on Docker Desktop for Windows to ensure a fair comparison with identical virtualization overhead.

## Test Environment

```
BenchmarkDotNet v0.14.0
OS: Windows 11 (10.0.26200.7462)
CPU: Intel® Core™ Ultra 9 285HX (24 cores)
RAM: 64GB
Virtualization: Docker Desktop for Windows (both containers)
.NET SDK: 10.0.101
Runtime: .NET 8.0.23 (8.0.2325.60607), X64 RyuJIT AVX2
GC: Concurrent Workstation
```

### Container Images
- **Memorize**: `memorize:latest` (custom build, ~25MB)
- **Redis**: `redis:latest` (official image)
