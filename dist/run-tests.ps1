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
