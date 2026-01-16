# Run the Memorize server
param(
    [string]$Host = "127.0.0.1",
    [int]$Port = 50051,
    [int]$CleanupInterval = 60,
    [string]$ApiKey = ""
)

$env:MEMORIZE_HOST = $Host
$env:MEMORIZE_PORT = $Port
$env:MEMORIZE_CLEANUP_INTERVAL = $CleanupInterval
if ($ApiKey) {
    $env:MEMORIZE_API_KEY = $ApiKey
    Write-Host "Starting Memorize server on ${Host}:${Port} (auth enabled)..." -ForegroundColor Cyan
} else {
    Remove-Item Env:MEMORIZE_API_KEY -ErrorAction SilentlyContinue
    Write-Host "Starting Memorize server on ${Host}:${Port} (auth disabled)..." -ForegroundColor Yellow
}
& "$PSScriptRoot\bin\memorize-server.exe"
