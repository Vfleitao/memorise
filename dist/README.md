# Memorize - Build Output

## Contents

- `bin/memorize-server.exe` - The gRPC server
- `bin/memorize-integration-tests.exe` - Rust integration tests
- `bin/memorize-integration-tests-csharp.exe` - C# integration tests  
- `proto/memorize.proto` - Protocol Buffer definition (use this for your client)

## Quick Start

1. Start the server:
   `powershell
   ./run-server.ps1
   # Or with custom settings:
   ./run-server.ps1 -Host "0.0.0.0" -Port 50051 -CleanupInterval 120
   `

2. Run the tests (in another terminal):
   `powershell
   # Rust tests
   ./run-tests.ps1
   
   # C# tests
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

Built: 2026-01-16 16:07:40
Configuration: Release
