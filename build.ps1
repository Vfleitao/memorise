#Requires -Version 7.0
<#
.SYNOPSIS
    Build script for Memorize - outputs all artifacts needed for demos.

.DESCRIPTION
    This script builds the Rust projects and copies all necessary files
    to an output directory for use by demo applications in any language.

.PARAMETER Configuration
    Build configuration: Debug or Release. Default is Release.

.PARAMETER OutputDir
    Output directory for artifacts. Default is ./dist

.EXAMPLE
    ./build.ps1
    ./build.ps1 -Configuration Debug
    ./build.ps1 -Configuration Release -OutputDir ./artifacts
#>

param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    
    [string]$OutputDir = "./dist"
)

$ErrorActionPreference = "Stop"

# Colors for output
function Write-Step { param($Message) Write-Host "▶ $Message" -ForegroundColor Cyan }
function Write-Success { param($Message) Write-Host "✓ $Message" -ForegroundColor Green }
function Write-Error { param($Message) Write-Host "✗ $Message" -ForegroundColor Red }

# Banner
Write-Host ""
Write-Host "╔═══════════════════════════════════════════════════════════╗" -ForegroundColor Magenta
Write-Host "║                    Memorize Build Script                  ║" -ForegroundColor Magenta
Write-Host "╚═══════════════════════════════════════════════════════════╝" -ForegroundColor Magenta
Write-Host ""

$rootDir = $PSScriptRoot
$cargoProfile = if ($Configuration -eq "Release") { "release" } else { "debug" }
$cargoFlag = if ($Configuration -eq "Release") { "--release" } else { "" }

# Resolve output directory
$OutputDir = [System.IO.Path]::GetFullPath([System.IO.Path]::Combine($rootDir, $OutputDir))

Write-Host "Configuration: $Configuration" -ForegroundColor Yellow
Write-Host "Output Dir:    $OutputDir" -ForegroundColor Yellow
Write-Host ""

# Step 1: Find or set PROTOC
Write-Step "Locating protoc compiler..."

if (-not $env:PROTOC) {
    # Try common locations
    $protocPaths = @(
        "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\Google.Protobuf_Microsoft.Winget.Source_8wekyb3d8bbwe\bin\protoc.exe",
        "$env:ProgramFiles\protoc\bin\protoc.exe",
        "$env:USERPROFILE\scoop\apps\protobuf\current\bin\protoc.exe",
        "C:\ProgramData\chocolatey\bin\protoc.exe"
    )
    
    foreach ($path in $protocPaths) {
        if (Test-Path $path) {
            $env:PROTOC = $path
            break
        }
    }
    
    # Try PATH
    if (-not $env:PROTOC) {
        $protoc = Get-Command protoc -ErrorAction SilentlyContinue
        if ($protoc) {
            $env:PROTOC = $protoc.Source
        }
    }
}

if (-not $env:PROTOC -or -not (Test-Path $env:PROTOC)) {
    Write-Error "protoc not found! Please install it:"
    Write-Host "  winget install Google.Protobuf" -ForegroundColor Yellow
    exit 1
}

Write-Success "Found protoc: $env:PROTOC"

# Step 2: Build Rust projects
Write-Step "Building memorize-core..."
Push-Location "$rootDir\memorize-core"
cargo build $cargoFlag
if ($LASTEXITCODE -ne 0) { Write-Error "Failed to build memorize-core"; exit 1 }
Pop-Location
Write-Success "memorize-core built"

Write-Step "Building memorize-proto..."
Push-Location "$rootDir\memorize-proto"
cargo build $cargoFlag
if ($LASTEXITCODE -ne 0) { Write-Error "Failed to build memorize-proto"; exit 1 }
Pop-Location
Write-Success "memorize-proto built"

Write-Step "Building memorize-server..."
Push-Location "$rootDir\memorize-server"
cargo build $cargoFlag
if ($LASTEXITCODE -ne 0) { Write-Error "Failed to build memorize-server"; exit 1 }
Pop-Location
Write-Success "memorize-server built"

Write-Step "Building integration tests (Rust)..."
Push-Location "$rootDir\demo-apps\memorize-integration-tests"
cargo build $cargoFlag
if ($LASTEXITCODE -ne 0) { Write-Error "Failed to build integration tests"; exit 1 }
Pop-Location
Write-Success "Rust integration tests built"

# Step 3: Run tests
Write-Step "Running unit tests..."
Push-Location "$rootDir\memorize-core"
cargo test $cargoFlag
if ($LASTEXITCODE -ne 0) { Write-Error "Unit tests failed"; exit 1 }
Pop-Location
Write-Success "All unit tests passed"

# Step 4: Create output directory structure
Write-Step "Creating output directory..."

$dirs = @(
    "$OutputDir",
    "$OutputDir\bin",
    "$OutputDir\proto",
    "$OutputDir\include"
)

foreach ($dir in $dirs) {
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
}
Write-Success "Output directory created: $OutputDir"

# Step 5: Copy artifacts
Write-Step "Copying artifacts..."

# Server binary
$serverBinary = "$rootDir\memorize-server\target\$cargoProfile\memorize-server.exe"
Copy-Item $serverBinary "$OutputDir\bin\memorize-server.exe" -Force
Write-Success "Copied: memorize-server.exe"

# Integration test binary
$testBinary = "$rootDir\demo-apps\memorize-integration-tests\target\$cargoProfile\memorize-integration-tests.exe"
Copy-Item $testBinary "$OutputDir\bin\memorize-integration-tests.exe" -Force
Write-Success "Copied: memorize-integration-tests.exe"

# Proto file
Copy-Item "$rootDir\memorize-proto\proto\memorize.proto" "$OutputDir\proto\memorize.proto" -Force
Write-Success "Copied: memorize.proto"

# Step 5b: Build C# integration tests
Write-Step "Building C# integration tests..."
$csharpProjectDir = "$rootDir\demo-apps\memorize-integration-tests-csharp\MemorizeIntegrationTests"
$csharpProtosDir = "$csharpProjectDir\Protos"

# Ensure proto file is synced to C# project
if (-not (Test-Path $csharpProtosDir)) {
    New-Item -ItemType Directory -Path $csharpProtosDir -Force | Out-Null
}
Copy-Item "$OutputDir\proto\memorize.proto" "$csharpProtosDir\memorize.proto" -Force

$dotnetConfig = if ($Configuration -eq "Release") { "Release" } else { "Debug" }
Push-Location $csharpProjectDir
dotnet build --configuration $dotnetConfig
if ($LASTEXITCODE -ne 0) { Write-Error "Failed to build C# integration tests"; exit 1 }
Pop-Location
Write-Success "C# integration tests built"

# Copy C# binary
$csharpBinary = "$csharpProjectDir\bin\$dotnetConfig\net8.0\MemorizeIntegrationTests.exe"
if (Test-Path $csharpBinary) {
    Copy-Item $csharpBinary "$OutputDir\bin\memorize-integration-tests-csharp.exe" -Force
    Write-Success "Copied: memorize-integration-tests-csharp.exe"
}

# Step 6: Generate a run script
Write-Step "Generating helper scripts..."

$runServerScript = @'
# Run the Memorize server
param(
    [string]$Host = "127.0.0.1",
    [int]$Port = 50051,
    [int]$CleanupInterval = 60
)

$env:MEMORIZE_HOST = $Host
$env:MEMORIZE_PORT = $Port
$env:MEMORIZE_CLEANUP_INTERVAL = $CleanupInterval

Write-Host "Starting Memorize server on ${Host}:${Port}..." -ForegroundColor Cyan
& "$PSScriptRoot\bin\memorize-server.exe"
'@

$runServerScript | Out-File "$OutputDir\run-server.ps1" -Encoding UTF8
Write-Success "Created: run-server.ps1"

$runTestsScript = @'
# Run the integration tests
param(
    [string]$ServerUrl = "http://127.0.0.1:50051"
)

$env:MEMORIZE_SERVER_URL = $ServerUrl

Write-Host "Running integration tests against $ServerUrl..." -ForegroundColor Cyan
& "$PSScriptRoot\bin\memorize-integration-tests.exe"
'@

$runTestsScript | Out-File "$OutputDir\run-tests.ps1" -Encoding UTF8
Write-Success "Created: run-tests.ps1"

# Step 7: Create README
$readme = @"
# Memorize - Build Output

## Contents

- ``bin/memorize-server.exe`` - The gRPC server
- ``bin/memorize-integration-tests.exe`` - Rust integration tests
- ``bin/memorize-integration-tests-csharp.exe`` - C# integration tests  
- ``proto/memorize.proto`` - Protocol Buffer definition (use this for your client)

## Quick Start

1. Start the server:
   ```powershell
   ./run-server.ps1
   # Or with custom settings:
   ./run-server.ps1 -Host "0.0.0.0" -Port 50051 -CleanupInterval 120
   ```

2. Run the tests (in another terminal):
   ```powershell
   # Rust tests
   ./run-tests.ps1
   
   # C# tests
   ./bin/memorize-integration-tests-csharp.exe
   ```

## Using the Proto File

### C# / .NET
```xml
<ItemGroup>
  <Protobuf Include="proto\memorize.proto" GrpcServices="Client" />
</ItemGroup>
```

### Python
```bash
python -m grpc_tools.protoc -I./proto --python_out=. --grpc_python_out=. ./proto/memorize.proto
```

### Node.js
```bash
npx grpc-tools protoc --js_out=import_style=commonjs:. --grpc_out=. -I ./proto ./proto/memorize.proto
```

### Go
```bash
protoc --go_out=. --go-grpc_out=. -I ./proto ./proto/memorize.proto
```

## Server Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| MEMORIZE_HOST | 127.0.0.1 | Bind address |
| MEMORIZE_PORT | 50051 | Port number |
| MEMORIZE_CLEANUP_INTERVAL | 60 | Cleanup interval in seconds |

Built: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss")
Configuration: $Configuration
"@

$readme | Out-File "$OutputDir\README.md" -Encoding UTF8
Write-Success "Created: README.md"

# Done!
Write-Host ""
Write-Host "╔═══════════════════════════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║                    Build Complete! ✓                      ║" -ForegroundColor Green
Write-Host "╚═══════════════════════════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""
Write-Host "Output directory: $OutputDir" -ForegroundColor Yellow
Write-Host ""
Write-Host "Contents:" -ForegroundColor Cyan
Get-ChildItem $OutputDir -Recurse -File | ForEach-Object {
    $relativePath = $_.FullName.Substring($OutputDir.Length + 1)
    $size = "{0:N0} KB" -f ($_.Length / 1KB)
    Write-Host "  $relativePath ($size)"
}
Write-Host ""
