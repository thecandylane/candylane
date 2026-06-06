# Reverses example-tweak.ps1 — removes the marker it wrote. Idempotent: a missing marker
# is not an error, so revert is safe to run more than once.
$marker = Join-Path $env:USERPROFILE ".candylane-example-tweak"
if (Test-Path $marker) { Remove-Item $marker -Force }
Write-Host "removed $marker"
