# Example post-install tweak for candylane/minimal-dev.
# Paired with example-tweak.undo.ps1 so the action is `inverse` (fully reversible).
# It writes a harmless marker file; the undo script removes it. Replace with your own
# tweak — and always ship a matching undo, or the action becomes one-way.
$marker = Join-Path $env:USERPROFILE ".candylane-example-tweak"
Set-Content -Path $marker -Value "candylane minimal-dev post-install ran"
Write-Host "wrote $marker"
