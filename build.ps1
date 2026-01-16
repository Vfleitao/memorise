#Requires -Version 7.0
<#
.SYNOPSIS
    Build script for Memorize - a high-performance in-memory cache with gRPC API.

.DESCRIPTION
    This script performs the following steps:
    1. Build Core       - Builds the memorize-core library
    2. Build Proto      - Builds the memorize-proto library
    3. Build Server     - Builds the memorize-server gRPC API
    4. Run Tests        - Runs all Rust unit tests
    5. Distribute Proto - Copies .proto file to all locations that need it
    6. Build Apps       - Builds demo/integration test applications
    7. Build Bindings   - Builds language bindings (C#) and packages
    8. Package          - Copies all artifacts to the output directory

.PARAMETER Configuration
    Build configuration: Debug or Release. Default is Release.

.PARAMETER OutputDir
    Output directory for artifacts. Default is ./dist

.PARAMETER SkipTests
    Skip running unit tests.

.EXAMPLE
    ./build.ps1
    ./build.ps1 -Configuration Debug
    ./build.ps1 -Configuration Release -OutputDir ./artifacts
    ./build.ps1 -SkipTests
#>

param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    
    [string]$OutputDir = "./dist",
    
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"

#═══════════════════════════════════════════════════════════════════════════════
#region HELPER FUNCTIONS
#═══════════════════════════════════════════════════════════════════════════════

function Write-Banner { 
    param($Message) 
    Write-Host ""
    Write-Host "╔═══════════════════════════════════════════════════════════╗" -ForegroundColor Magenta
    Write-Host "║  $($Message.PadRight(55))  ║" -ForegroundColor Magenta
    Write-Host "╚═══════════════════════════════════════════════════════════╝" -ForegroundColor Magenta
    Write-Host ""
}

function Write-Section { 
    param($Number, $Title) 
    Write-Host ""
    Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Blue
    Write-Host " STEP $Number : $Title" -ForegroundColor Blue
    Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Blue
}

function Write-Step { param($Message) Write-Host "  ▶ $Message" -ForegroundColor Cyan }
function Write-Success { param($Message) Write-Host "  ✓ $Message" -ForegroundColor Green }
function Write-Warning { param($Message) Write-Host "  ⚠ $Message" -ForegroundColor Yellow }
function Write-Err { param($Message) Write-Host "  ✗ $Message" -ForegroundColor Red }

function Invoke-BuildStep {
    param(
        [string]$Name,
        [scriptblock]$Action
    )
    Write-Step $Name
    try {
        & $Action
        if ($LASTEXITCODE -ne 0) { throw "Command failed with exit code $LASTEXITCODE" }
        Write-Success $Name
    }
    catch {
        Write-Err "Failed: $Name"
        Write-Host "    $_" -ForegroundColor Red
        exit 1
    }
}

#endregion

#═══════════════════════════════════════════════════════════════════════════════
#region CONFIGURATION
#═══════════════════════════════════════════════════════════════════════════════

$rootDir = $PSScriptRoot
$cargoProfile = if ($Configuration -eq "Release") { "release" } else { "debug" }
$cargoFlag = if ($Configuration -eq "Release") { "--release" } else { "" }
$dotnetConfig = if ($Configuration -eq "Release") { "Release" } else { "Debug" }
$OutputDir = [System.IO.Path]::GetFullPath([System.IO.Path]::Combine($rootDir, $OutputDir))

# Project paths
$paths = @{
    # Core Rust projects
    Core        = "$rootDir\memorize-core"
    Proto       = "$rootDir\memorize-proto"
    Server      = "$rootDir\memorize-server"
    
    # Language bindings - Rust
    RustClient  = "$rootDir\bindings\rust\memorize-client"
    RustTests   = "$rootDir\bindings\rust\memorize-integration-tests"
    
    # Language bindings - C#
    CSharpClient = "$rootDir\bindings\csharp\Memorize.Client"
    CSharpTests  = "$rootDir\bindings\csharp\Memorize.IntegrationTests"
}

# Proto file source
$protoSource = "$($paths.Proto)\proto\memorize.proto"

#endregion

Write-Banner "Memorize Build Script"

Write-Host "Configuration: $Configuration" -ForegroundColor Yellow
Write-Host "Output Dir:    $OutputDir" -ForegroundColor Yellow

#═══════════════════════════════════════════════════════════════════════════════
#region PREREQUISITES
#═══════════════════════════════════════════════════════════════════════════════

Write-Section 0 "Prerequisites"

Write-Step "Locating protoc compiler..."
if (-not $env:PROTOC) {
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
    
    if (-not $env:PROTOC) {
        $protoc = Get-Command protoc -ErrorAction SilentlyContinue
        if ($protoc) { $env:PROTOC = $protoc.Source }
    }
}

if (-not $env:PROTOC -or -not (Test-Path $env:PROTOC)) {
    Write-Err "protoc not found! Install with: winget install Google.Protobuf"
    exit 1
}
Write-Success "Found protoc: $env:PROTOC"

#endregion

#═══════════════════════════════════════════════════════════════════════════════
#region STEP 1: BUILD CORE
#═══════════════════════════════════════════════════════════════════════════════

Write-Section 1 "Build Core Library"

Invoke-BuildStep "memorize-core (in-memory storage engine)" {
    Push-Location $paths.Core
    cargo build $cargoFlag
    Pop-Location
}

#endregion

#═══════════════════════════════════════════════════════════════════════════════
#region STEP 2: BUILD PROTO
#═══════════════════════════════════════════════════════════════════════════════

Write-Section 2 "Build Proto Library"

Invoke-BuildStep "memorize-proto (gRPC protocol definitions)" {
    Push-Location $paths.Proto
    cargo build $cargoFlag
    Pop-Location
}

#endregion

#═══════════════════════════════════════════════════════════════════════════════
#region STEP 3: BUILD SERVER
#═══════════════════════════════════════════════════════════════════════════════

Write-Section 3 "Build Server (gRPC API)"

Invoke-BuildStep "memorize-server (gRPC server)" {
    Push-Location $paths.Server
    cargo build $cargoFlag
    Pop-Location
}

#endregion

#═══════════════════════════════════════════════════════════════════════════════
#region STEP 4: RUN TESTS
#═══════════════════════════════════════════════════════════════════════════════

Write-Section 4 "Run Rust Unit Tests"

if ($SkipTests) {
    Write-Warning "Skipping tests (-SkipTests flag)"
} else {
    Invoke-BuildStep "memorize-core unit tests" {
        Push-Location $paths.Core
        cargo test $cargoFlag
        Pop-Location
    }
    
    Invoke-BuildStep "memorize-server unit tests" {
        Push-Location $paths.Server
        cargo test $cargoFlag
        Pop-Location
    }
}

#endregion

#═══════════════════════════════════════════════════════════════════════════════
#region STEP 5: DISTRIBUTE PROTO
#═══════════════════════════════════════════════════════════════════════════════

Write-Section 5 "Distribute Proto Files"

# Ensure output directories exist
$dirs = @("$OutputDir", "$OutputDir\bin", "$OutputDir\proto", "$OutputDir\nuget")
foreach ($dir in $dirs) {
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
}

Write-Step "Copying memorize.proto to distribution locations..."

# Copy to dist output
Copy-Item $protoSource "$OutputDir\proto\memorize.proto" -Force
Write-Success "  → $OutputDir\proto\"

# Copy to C# bindings
$csharpProtosDir = "$($paths.CSharpClient)\Protos"
if (-not (Test-Path $csharpProtosDir)) {
    New-Item -ItemType Directory -Path $csharpProtosDir -Force | Out-Null
}
Copy-Item $protoSource "$csharpProtosDir\memorize.proto" -Force
Write-Success "  → bindings\csharp\Memorize.Client\Protos\"

# Copy to Rust bindings
$rustProtosDir = "$($paths.RustClient)\proto"
if (-not (Test-Path $rustProtosDir)) {
    New-Item -ItemType Directory -Path $rustProtosDir -Force | Out-Null
}
Copy-Item $protoSource "$rustProtosDir\memorize.proto" -Force
Write-Success "  → bindings\rust\memorize-client\proto\"

# Future: Copy to other language bindings here
# Copy-Item $protoSource "$($paths.PythonClient)\protos\memorize.proto" -Force
# Copy-Item $protoSource "$($paths.GoClient)\proto\memorize.proto" -Force

#endregion

#═══════════════════════════════════════════════════════════════════════════════
#region STEP 6: BUILD BINDINGS
#═══════════════════════════════════════════════════════════════════════════════

Write-Section 6 "Build Language Bindings"

#───────────────────────────────────────────────────────────────────────────────
# Rust Bindings
#───────────────────────────────────────────────────────────────────────────────
Write-Host "  ── Rust ──" -ForegroundColor DarkCyan

Invoke-BuildStep "memorize-client (Cargo crate)" {
    Push-Location $paths.RustClient
    cargo build $cargoFlag
    Pop-Location
}

Invoke-BuildStep "memorize-integration-tests" {
    Push-Location $paths.RustTests
    cargo build $cargoFlag
    Pop-Location
}

#───────────────────────────────────────────────────────────────────────────────
# C# Bindings
#───────────────────────────────────────────────────────────────────────────────
Write-Host "  ── C# ──" -ForegroundColor DarkCyan

Invoke-BuildStep "Memorize.Client (NuGet library)" {
    Push-Location $paths.CSharpClient
    dotnet build --configuration $dotnetConfig
    Pop-Location
}

Invoke-BuildStep "Memorize.IntegrationTests" {
    Push-Location $paths.CSharpTests
    dotnet build --configuration $dotnetConfig
    Pop-Location
}

Invoke-BuildStep "NuGet package" {
    Push-Location $paths.CSharpClient
    dotnet pack --configuration $dotnetConfig --output "$OutputDir\nuget" --no-build
    Pop-Location
}

# Future: Add more language bindings here
#───────────────────────────────────────────────────────────────────────────────
# Python Bindings (placeholder)
#───────────────────────────────────────────────────────────────────────────────
# Write-Host "  ── Python ──" -ForegroundColor DarkCyan
# Invoke-BuildStep "memorize-py" { ... }

#───────────────────────────────────────────────────────────────────────────────
# Go Bindings (placeholder)
#───────────────────────────────────────────────────────────────────────────────
# Write-Host "  ── Go ──" -ForegroundColor DarkCyan
# Invoke-BuildStep "memorize-go" { ... }

#endregion

#═══════════════════════════════════════════════════════════════════════════════
#region STEP 7: PACKAGE ARTIFACTS
#═══════════════════════════════════════════════════════════════════════════════

Write-Section 7 "Package Artifacts"

Write-Step "Copying binaries to dist..."

# Server binary
Copy-Item "$($paths.Server)\target\$cargoProfile\memorize-server.exe" "$OutputDir\bin\" -Force
Write-Success "  → bin\memorize-server.exe"

# Rust integration tests
Copy-Item "$($paths.RustTests)\target\$cargoProfile\memorize-integration-tests.exe" "$OutputDir\bin\" -Force
Write-Success "  → bin\memorize-integration-tests.exe"

# C# integration tests (need all runtime dependencies)
$csharpBuildDir = "$($paths.CSharpTests)\bin\$dotnetConfig\net8.0"
if (Test-Path "$csharpBuildDir\memorize-integration-tests-csharp.exe") {
    Copy-Item "$csharpBuildDir\*" "$OutputDir\bin\" -Force -Exclude "*.pdb","*.xml"
    Write-Success "  → bin\memorize-integration-tests-csharp.exe"
}

#───────────────────────────────────────────────────────────────────────────────
# Generate helper scripts
#───────────────────────────────────────────────────────────────────────────────
Write-Step "Generating helper scripts..."

@'
# Run the Memorize server
param(
    [string]$ServerHost = "127.0.0.1",
    [int]$Port = 50051,
    [int]$CleanupInterval = 60,
    [string]$ApiKey = ""
)

$env:MEMORIZE_HOST = $ServerHost
$env:MEMORIZE_PORT = $Port
$env:MEMORIZE_CLEANUP_INTERVAL = $CleanupInterval
if ($ApiKey) {
    $env:MEMORIZE_API_KEY = $ApiKey
    Write-Host "Starting Memorize server on ${ServerHost}:${Port} (auth enabled)..." -ForegroundColor Cyan
} else {
    Remove-Item Env:MEMORIZE_API_KEY -ErrorAction SilentlyContinue
    Write-Host "Starting Memorize server on ${ServerHost}:${Port} (auth disabled)..." -ForegroundColor Yellow
}
& "$PSScriptRoot\bin\memorize-server.exe"
'@ | Out-File "$OutputDir\run-server.ps1" -Encoding UTF8
Write-Success "  → run-server.ps1"

@'
# Run the integration tests (Rust and C#)
param(
    [string]$ServerUrl = "http://127.0.0.1:50051",
    [string]$ApiKey = ""
)

$env:MEMORIZE_SERVER_URL = $ServerUrl
if ($ApiKey) { $env:MEMORIZE_API_KEY = $ApiKey }

Write-Host ""
Write-Host "Running integration tests against $ServerUrl..." -ForegroundColor Cyan
Write-Host ""

Write-Host "=== Rust Integration Tests ===" -ForegroundColor Magenta
& "$PSScriptRoot\bin\memorize-integration-tests.exe"
$rustResult = $LASTEXITCODE

Write-Host ""
Write-Host "=== C# Integration Tests ===" -ForegroundColor Magenta
& "$PSScriptRoot\bin\memorize-integration-tests-csharp.exe"
$csharpResult = $LASTEXITCODE

Write-Host ""
if ($rustResult -eq 0 -and $csharpResult -eq 0) {
    Write-Host "All integration tests passed!" -ForegroundColor Green
    exit 0
} else {
    Write-Host "Some tests failed!" -ForegroundColor Red
    exit 1
}
'@ | Out-File "$OutputDir\run-tests.ps1" -Encoding UTF8
Write-Success "  → run-tests.ps1"

#───────────────────────────────────────────────────────────────────────────────
# Generate README
#───────────────────────────────────────────────────────────────────────────────
Write-Step "Generating README..."
@"
# Memorize - Build Output

Built: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss")
Configuration: $Configuration

## Contents

| File | Description |
|------|-------------|
| ``bin/memorize-server.exe`` | The gRPC cache server |
| ``bin/memorize-integration-tests.exe`` | Rust integration tests |
| ``bin/memorize-integration-tests-csharp.exe`` | C# integration tests |
| ``proto/memorize.proto`` | Protocol Buffer definition |
| ``nuget/Memorize.Client.*.nupkg`` | C# client library NuGet package |

## Quick Start

``````powershell
# Start the server
./run-server.ps1

# With authentication
./run-server.ps1 -ApiKey "your-secret-key"

# In another terminal, run tests
./run-tests.ps1
./run-tests.ps1 -ApiKey "your-secret-key"
``````

## Using the C# Client Library

``````powershell
# Install from local NuGet
dotnet add package Memorize.Client --source ./nuget
``````

``````csharp
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
``````

## Server Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| ``MEMORIZE_HOST`` | 127.0.0.1 | Bind address |
| ``MEMORIZE_PORT`` | 50051 | Port number |
| ``MEMORIZE_CLEANUP_INTERVAL`` | 60 | Cleanup interval (seconds) |
| ``MEMORIZE_API_KEY`` | (none) | API key (if unset, auth disabled) |
"@ | Out-File "$OutputDir\README.md" -Encoding UTF8
Write-Success "  → README.md"

#endregion

#═══════════════════════════════════════════════════════════════════════════════
#region BUILD SUMMARY
#═══════════════════════════════════════════════════════════════════════════════

Write-Host ""
Write-Host "╔═══════════════════════════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║                    Build Complete! ✓                      ║" -ForegroundColor Green
Write-Host "╚═══════════════════════════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""
Write-Host "Output Directory: $OutputDir" -ForegroundColor Yellow
Write-Host ""

$nupkg = Get-ChildItem "$OutputDir\nuget\*.nupkg" -ErrorAction SilentlyContinue | Select-Object -First 1

Write-Host "Artifacts:" -ForegroundColor Cyan
Write-Host "  bin\"
Write-Host "    memorize-server.exe"
Write-Host "    memorize-integration-tests.exe"
Write-Host "    memorize-integration-tests-csharp.exe"
Write-Host "  proto\"
Write-Host "    memorize.proto"
Write-Host "  nuget\"
if ($nupkg) { Write-Host "    $($nupkg.Name)" }
Write-Host ""
Write-Host "Run './dist/run-server.ps1' to start the server" -ForegroundColor DarkGray
Write-Host ""

#endregion
