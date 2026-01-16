#Requires -Version 7.0
<#
.SYNOPSIS
    Build Docker image for Memorize server.

.DESCRIPTION
    This script builds a Docker image containing the Memorize gRPC server.
    The image is built using a multi-stage Dockerfile for minimal size.

.PARAMETER ImageName
    Name for the Docker image. Default is "memorize-server".

.PARAMETER Tag
    Tag for the Docker image. Default is "latest".

.PARAMETER Push
    If specified, push the image to the registry after building.

.PARAMETER Registry
    Docker registry to push to (e.g., "ghcr.io/username" or "docker.io/username").

.PARAMETER NoBuildCache
    If specified, build without using Docker cache.

.EXAMPLE
    ./build-docker.ps1
    ./build-docker.ps1 -Tag "1.0.0"
    ./build-docker.ps1 -ImageName "my-memorize" -Tag "dev"
    ./build-docker.ps1 -Registry "ghcr.io/myuser" -Tag "1.0.0" -Push
#>

param(
    [string]$ImageName = "memorize-server",
    [string]$Tag = "latest",
    [switch]$Push,
    [string]$Registry = "",
    [switch]$NoBuildCache
)

$ErrorActionPreference = "Stop"

# Colors for output
function Write-Step { param($Message) Write-Host "▶ $Message" -ForegroundColor Cyan }
function Write-Success { param($Message) Write-Host "✓ $Message" -ForegroundColor Green }
function Write-Error { param($Message) Write-Host "✗ $Message" -ForegroundColor Red }

# Banner
Write-Host ""
Write-Host "╔═══════════════════════════════════════════════════════════╗" -ForegroundColor Magenta
Write-Host "║              Memorize Docker Build Script                 ║" -ForegroundColor Magenta
Write-Host "╚═══════════════════════════════════════════════════════════╝" -ForegroundColor Magenta
Write-Host ""

$rootDir = $PSScriptRoot

# Determine full image name
$fullImageName = if ($Registry) { "$Registry/$ImageName" } else { $ImageName }
$fullImageTag = "${fullImageName}:${Tag}"

Write-Host "Image:     $fullImageTag" -ForegroundColor Yellow
Write-Host ""

# Check Docker is available
Write-Step "Checking Docker..."
try {
    $dockerVersion = docker version --format '{{.Server.Version}}' 2>$null
    if (-not $dockerVersion) {
        throw "Docker not responding"
    }
    Write-Success "Docker version: $dockerVersion"
} catch {
    Write-Error "Docker is not available. Please ensure Docker is installed and running."
    exit 1
}

# Build the image
Write-Step "Building Docker image..."

$buildArgs = @(
    "build"
    "-t", $fullImageTag
    "-f", "$rootDir\Dockerfile"
)

if ($NoBuildCache) {
    $buildArgs += "--no-cache"
}

$buildArgs += $rootDir

Write-Host "Running: docker $($buildArgs -join ' ')" -ForegroundColor Gray

& docker @buildArgs

if ($LASTEXITCODE -ne 0) {
    Write-Error "Docker build failed"
    exit 1
}

Write-Success "Docker image built: $fullImageTag"

# Get image size
$imageSize = docker image inspect $fullImageTag --format '{{.Size}}' 2>$null
if ($imageSize) {
    $imageSizeMB = [math]::Round($imageSize / 1MB, 1)
    Write-Host "   Image size: ${imageSizeMB} MB" -ForegroundColor Gray
}

# Also tag as latest if we're building a specific version
if ($Tag -ne "latest") {
    $latestTag = "${fullImageName}:latest"
    docker tag $fullImageTag $latestTag
    Write-Success "Also tagged as: $latestTag"
}

# Push if requested
if ($Push) {
    if (-not $Registry) {
        Write-Error "Cannot push without specifying -Registry"
        exit 1
    }
    
    Write-Step "Pushing image to registry..."
    docker push $fullImageTag
    
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Docker push failed"
        exit 1
    }
    
    Write-Success "Pushed: $fullImageTag"
    
    if ($Tag -ne "latest") {
        docker push $latestTag
        Write-Success "Pushed: $latestTag"
    }
}

# Done!
Write-Host ""
Write-Host "╔═══════════════════════════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║                  Docker Build Complete! ✓                 ║" -ForegroundColor Green
Write-Host "╚═══════════════════════════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""

# Print usage instructions
Write-Host "To run the container:" -ForegroundColor Cyan
Write-Host ""
Write-Host "  # Without authentication (development)" -ForegroundColor Gray
Write-Host "  docker run -p 50051:50051 $fullImageTag" -ForegroundColor White
Write-Host ""
Write-Host "  # With authentication (recommended)" -ForegroundColor Gray
Write-Host "  docker run -p 50051:50051 -e MEMORIZE_API_KEY=your-secret-key $fullImageTag" -ForegroundColor White
Write-Host ""
Write-Host "  # With all options" -ForegroundColor Gray
Write-Host "  docker run -p 50051:50051 \" -ForegroundColor White
Write-Host "    -e MEMORIZE_API_KEY=your-secret-key \" -ForegroundColor White
Write-Host "    -e MEMORIZE_CLEANUP_INTERVAL=120 \" -ForegroundColor White
Write-Host "    $fullImageTag" -ForegroundColor White
Write-Host ""
Write-Host "Environment variables:" -ForegroundColor Cyan
Write-Host "  MEMORIZE_HOST              Bind address (default: 0.0.0.0)" -ForegroundColor Gray
Write-Host "  MEMORIZE_PORT              Port number (default: 50051)" -ForegroundColor Gray
Write-Host "  MEMORIZE_CLEANUP_INTERVAL  Cleanup interval in seconds (default: 60)" -ForegroundColor Gray
Write-Host "  MEMORIZE_API_KEY           API key for auth (if not set, auth disabled)" -ForegroundColor Gray
Write-Host ""
