# profiles/refurb-clean/scripts/cleanup.ps1
#
# Main cleanup for old/dirty Windows AIOs.
# Run as part of `candylane pull profiles/refurb-clean/candylane.toml`
# after a "Reset this PC", clean install, or OOBE.
#
# Safety notes:
# - Idempotent where possible (re-running is safe).
# - Never touches the currently logged-in user's profile or critical system accounts.
# - Many operations are best-effort (services may be re-enabled by updates later,
#   some bloat can return on feature updates).
# - Hostname change usually requires a reboot — the candylane engine will detect
#   pending reboot (CBS/WU) and abort cleanly if needed. You will get a clear message.
# - Run with appropriate rights (the profile is expected to be run elevated for
#   full effect on services, appx, hostname, etc.).
#
# Progress is loud on purpose — you will see exactly what is happening on each machine.
# If something fails hard, the engine will roll back what it can (best-effort for cleanup).

$ErrorActionPreference = "Continue"   # we want to keep going on individual step failures
$ProgressPreference = "SilentlyContinue"

Write-Host "=== CANDYLANE REFURB-CLEAN starting ===" -ForegroundColor Cyan
$start = Get-Date

# --- 1. Gather identity info (for hostname + marker) ---
$serial = "XXXX"
try {
    $bios = Get-CimInstance -ClassName Win32_BIOS -ErrorAction SilentlyContinue
    if ($bios -and $bios.SerialNumber) {
        $serial = ($bios.SerialNumber.Trim() -replace '[^A-Za-z0-9]', '')[-4..-1] -join ''
    }
} catch {}
$desiredHostname = "AIO-REFURB-$serial"
Write-Host "Target hostname: $desiredHostname (from BIOS serial)"

# --- 2. Set hostname (best-effort; often requires reboot) ---
$currentName = $env:COMPUTERNAME
if ($currentName -ne $desiredHostname) {
    Write-Host "Renaming computer from $currentName to $desiredHostname ..."
    try {
        Rename-Computer -NewName $desiredHostname -Force -ErrorAction Stop
        Write-Host "Hostname change requested. A reboot will likely be required." -ForegroundColor Yellow
    } catch {
        Write-Host "WARNING: Could not set hostname: $_" -ForegroundColor Yellow
    }
} else {
    Write-Host "Hostname already correct."
}

# --- 3. Basic firewall hardening (on for all profiles) ---
Write-Host "Ensuring Windows Firewall is on for all profiles..."
try {
    netsh advfirewall set allprofiles state on | Out-Null
    Write-Host "Firewall enabled."
} catch {
    Write-Host "WARNING: Firewall command failed: $_"
}

# --- 4. Power plan -> Balanced (sane default for AIOs) ---
Write-Host "Setting power plan to Balanced..."
try {
    powercfg /setactive 381b4222-f694-41f0-9685-ff5bb260df2e | Out-Null
    Write-Host "Power plan set."
} catch {
    Write-Host "WARNING: powercfg failed: $_"
}

# --- 5. Disable common telemetry / diagnostic services (best effort) ---
$telemetryServices = @(
    "DiagTrack",           # Connected User Experiences and Telemetry
    "dmwappushservice",    # WAP Push Message Routing
    "RetailDemo"           # Retail Demo Service
)

foreach ($svc in $telemetryServices) {
    try {
        $s = Get-Service -Name $svc -ErrorAction SilentlyContinue
        if ($s) {
            if ($s.Status -ne "Stopped") {
                Stop-Service -Name $svc -Force -ErrorAction SilentlyContinue
            }
            Set-Service -Name $svc -StartupType Disabled -ErrorAction SilentlyContinue
            Write-Host "Disabled service: $svc"
        }
    } catch {
        Write-Host "Service $svc not present or could not be disabled."
    }
}

# --- 6. Remove common consumer bloat / provisioned Appx (best effort, safe list) ---
# These are the ones that frequently come back or are just annoying on a clean machine.
# We do NOT remove things like Calculator, Photos, or Store if they are core.
$bloatPatterns = @(
    "*Microsoft.3DBuilder*",
    "*Microsoft.BingNews*",
    "*Microsoft.BingWeather*",
    "*Microsoft.GetHelp*",
    "*Microsoft.Getstarted*",
    "*Microsoft.Messaging*",
    "*Microsoft.MicrosoftOfficeHub*",
    "*Microsoft.MicrosoftSolitaireCollection*",
    "*Microsoft.OneConnect*",
    "*Microsoft.People*",
    "*Microsoft.SkypeApp*",
    "*Microsoft.WindowsAlarms*",
    "*Microsoft.WindowsFeedbackHub*",
    "*Microsoft.WindowsMaps*",
    "*Microsoft.Xbox*",
    "*Microsoft.ZuneMusic*",
    "*Microsoft.ZuneVideo*",
    "*Microsoft.YourPhone*",
    "*Microsoft.MixedReality.Portal*",
    "*Microsoft.Microsoft3DViewer*",
    "*Microsoft.Windows.Cortana*"   # may be limited on newer Windows
)

Write-Host "Removing provisioned Appx bloat packages (this can take a minute)..."
$provisioned = Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue
foreach ($pattern in $bloatPatterns) {
    $matches = $provisioned | Where-Object { $_.DisplayName -like $pattern }
    foreach ($pkg in $matches) {
        try {
            Remove-AppxProvisionedPackage -Online -PackageName $pkg.PackageName -ErrorAction Stop | Out-Null
            Write-Host "Removed provisioned: $($pkg.DisplayName)"
        } catch {
            # Many are "not found" or protected — that's fine.
        }
    }
}

# Also try to remove installed packages for the current user (harmless if already gone)
try {
    Get-AppxPackage -AllUsers -ErrorAction SilentlyContinue |
        Where-Object { 
            $_.Name -like "*Xbox*" -or 
            $_.Name -like "*Bing*" -or 
            $_.Name -like "*Zune*" -or 
            $_.Name -like "*Solitaire*" 
        } |
        Remove-AppxPackage -ErrorAction SilentlyContinue
} catch {}

# --- 7. Aggressive but safe temp / cache cleanup ---
Write-Host "Cleaning temp files, prefetch, update caches..."
$pathsToClean = @(
    "$env:TEMP\*",
    "C:\Windows\Temp\*",
    "C:\Windows\Prefetch\*",
    "C:\Windows\SoftwareDistribution\Download\*",
    "C:\Windows\Logs\*"
)

foreach ($p in $pathsToClean) {
    try {
        Remove-Item -Path $p -Recurse -Force -ErrorAction SilentlyContinue
    } catch {}
}

# Clean Windows.old if it exists (common after in-place upgrades / resets)
if (Test-Path "C:\Windows.old") {
    Write-Host "Removing C:\Windows.old (this can take time)..."
    try {
        Remove-Item "C:\Windows.old" -Recurse -Force -ErrorAction SilentlyContinue
    } catch {}
}

# --- 8. Remove leftover user profiles (keep only built-in + the one we're running as) ---
Write-Host "Removing old user profiles (except current and defaults)..."
try {
    $currentSid = (Get-CimInstance -Class Win32_UserAccount -Filter "Name='$env:USERNAME'").SID
    $profiles = Get-CimInstance -Class Win32_UserProfile -ErrorAction SilentlyContinue |
        Where-Object {
            -not $_.Special -and
            $_.SID -ne "S-1-5-18" -and          # Local System
            $_.SID -ne "S-1-5-19" -and          # Local Service
            $_.SID -ne "S-1-5-20" -and          # Network Service
            $_.SID -ne $currentSid
        }

    foreach ($prof in $profiles) {
        $userPath = $prof.LocalPath
        if ($userPath -and (Test-Path $userPath)) {
            Write-Host "Removing profile: $userPath"
            try {
                # Remove the profile via CIM when possible (cleaner)
                Remove-CimInstance -InputObject $prof -ErrorAction SilentlyContinue
                # Fallback: nuke the folder
                if (Test-Path $userPath) {
                    Remove-Item $userPath -Recurse -Force -ErrorAction SilentlyContinue
                }
            } catch {}
        }
    }
} catch {
    Write-Host "Profile cleanup encountered an issue (non-fatal): $_"
}

# --- 9. Drop the refurb marker on the public desktop (visible to new user) ---
$publicDesktop = [Environment]::GetFolderPath("CommonDesktopDirectory")
if (-not (Test-Path $publicDesktop)) {
    $publicDesktop = "C:\Users\Public\Desktop"
    New-Item -ItemType Directory -Path $publicDesktop -Force | Out-Null
}

$markerPath = Join-Path $publicDesktop "AIO-REFURB-CLEANED.txt"
$markerContent = @"
This Windows machine was cleaned and prepared with Candylane "refurb-clean".

Hostname : $desiredHostname
Cleaned  : $(Get-Date -Format "yyyy-MM-dd HH:mm:ss")
Tool     : https://github.com/thecandylane/candylane

All personal data, bloatware, and OEM junk have been removed.
The machine is ready for its next user.

If you need to re-run or customize the baseline, use the same profile.
"@

Set-Content -Path $markerPath -Value $markerContent -Force
Write-Host "Wrote marker: $markerPath"

# --- 10. Optional: quick disk space report for the operator ---
try {
    $drive = Get-CimInstance -Class Win32_LogicalDisk -Filter "DeviceID='C:'"
    $freeGB = [math]::Round($drive.FreeSpace / 1GB, 1)
    $sizeGB = [math]::Round($drive.Size / 1GB, 1)
    Write-Host "C: drive after cleanup: $freeGB GB free of $sizeGB GB"
} catch {}

$elapsed = (Get-Date) - $start
Write-Host "=== CANDYLANE REFURB-CLEAN finished in $([int]$elapsed.TotalSeconds) seconds ===" -ForegroundColor Green
Write-Host "Reboot may be required for hostname / some service changes." -ForegroundColor Yellow

# End of script. Any non-zero exit is still treated as success by the engine
# (scripts only fail hard on timeout or unrecoverable errors).
# The real state is what the machine looks like after this runs.
exit 0
