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
