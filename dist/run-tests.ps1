# Run the integration tests
param(
    [string]$ServerUrl = "http://127.0.0.1:50051"
)

$env:MEMORIZE_SERVER_URL = $ServerUrl

Write-Host "Running integration tests against $ServerUrl..." -ForegroundColor Cyan
& "$PSScriptRoot\bin\memorize-integration-tests.exe"
