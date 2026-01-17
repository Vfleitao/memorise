# Memorize - Build Output

## Contents

- in/memorize-server.exe - The gRPC server
- in/memorize-integration-tests.exe - Rust integration tests
- in/memorize-integration-tests-csharp.exe - C# integration tests  
- proto/memorize.proto - Protocol Buffer definition (use this for your client)

## Quick Start

1. Start the server:
   `powershell
   # Without authentication (development)
   ./run-server.ps1
   
   # With authentication (recommended for production)
   ./run-server.ps1 -ApiKey "your-secret-key"
   
   # Full options:
   ./run-server.ps1 -Host "0.0.0.0" -Port 50051 -CleanupInterval 120 -ApiKey "your-secret-key"
   `

2. Run the tests (in another terminal):
   `powershell
   # Rust tests
   ./run-tests.ps1
   # Or with auth:
   ./run-tests.ps1 -ApiKey "your-secret-key"
   
   # C# tests (set env var first if using auth)
   $env:MEMORIZE_API_KEY = "your-secret-key"
   ./bin/memorize-integration-tests-csharp.exe
   `

## Using the Proto File

### C# / .NET
`xml
<ItemGroup>
  <Protobuf Include="proto\memorize.proto" GrpcServices="Client" />
</ItemGroup>
`

### Python
`ash
python -m grpc_tools.protoc -I./proto --python_out=. --grpc_python_out=. ./proto/memorize.proto
`

### Node.js
`ash
npx grpc-tools protoc --js_out=import_style=commonjs:. --grpc_out=. -I ./proto ./proto/memorize.proto
`

### Go
`ash
protoc --go_out=. --go-grpc_out=. -I ./proto ./proto/memorize.proto
`

## Server Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| MEMORIZE_HOST | 127.0.0.1 | Bind address |
| MEMORIZE_PORT | 50051 | Port number |
| MEMORIZE_CLEANUP_INTERVAL | 60 | Cleanup interval in seconds |
| MEMORIZE_API_KEY | (none) | API key for authentication (if not set, auth is disabled) |

## Authentication

When MEMORIZE_API_KEY is set, all clients must include the x-api-key header with the matching value.
If not set, authentication is disabled and all requests are allowed.

Built: 2026-01-16 20:18:44
Configuration: Release
