# profiles/refurb-clean/scripts/cleanup.undo.ps1
#
# Best-effort undo for the refurb-clean profile.
#
# IMPORTANT REALITY CHECK (honest reversibility):
# - A deep clean (removing user data, bloat, old profiles, temp caches) is
#   mostly ONE-WAY by nature. You cannot magically restore the personal files
#   or OEM bloat you intentionally deleted.
# - This script only does the safe, reversible pieces we explicitly touched:
#     * Re-enable the telemetry/diagnostic services we disabled (to Automatic)
#     * Remove the "AIO-REFURB-CLEANED.txt" marker we dropped
# - Everything else (hostname may have changed, profiles gone, appx removed,
#   temp files deleted) is intentionally left as-is or logged as residue.
# - When you run `candylane revert` or `candylane recover`, the engine will
#   mark these as UndoSkipped or best_effort and continue. diff/history will
#   tell the truth.
#
# Use this undo primarily as a "whoops, I pulled the wrong profile" safety net
# on a machine you haven't yet handed to a customer.

$ErrorActionPreference = "Continue"

Write-Host "=== CANDYLANE REFURB-CLEAN best-effort UNDO starting ===" -ForegroundColor Yellow

# 1. Re-enable the services we touched (best effort — updates may change them again)
$servicesToRestore = @("DiagTrack", "dmwappushservice", "RetailDemo")

foreach ($svcName in $servicesToRestore) {
    try {
        $svc = Get-Service -Name $svcName -ErrorAction SilentlyContinue
        if ($svc) {
            Set-Service -Name $svcName -StartupType Automatic -ErrorAction SilentlyContinue
            if ($svc.Status -ne "Running") {
                Start-Service -Name $svcName -ErrorAction SilentlyContinue
            }
            Write-Host "Restored service: $svcName"
        }
    } catch {
        Write-Host "Could not restore $svcName (may not exist on this edition)"
    }
}

# 2. Remove the marker we created (idempotent)
$publicDesktop = [Environment]::GetFolderPath("CommonDesktopDirectory")
if (-not (Test-Path $publicDesktop)) {
    $publicDesktop = "C:\Users\Public\Desktop"
}
$marker = Join-Path $publicDesktop "AIO-REFURB-CLEANED.txt"

if (Test-Path $marker) {
    Remove-Item $marker -Force -ErrorAction SilentlyContinue
    Write-Host "Removed marker: $marker"
} else {
    Write-Host "Marker not present (already gone or never created on this run)."
}

# 3. Optional: if we want to be extra nice, we could try to set hostname back,
#    but we don't know the *original* name (it wasn't captured in Phase 1 state
#    for this action). So we leave it. The operator can rename manually.
Write-Host "Note: Hostname change (if any) is not automatically reverted."
Write-Host "      Original name was not recorded for this one-way step."

Write-Host "=== CANDYLANE REFURB-CLEAN UNDO finished (best-effort) ===" -ForegroundColor Yellow
exit 0
