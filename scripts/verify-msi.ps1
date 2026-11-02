param(
    [Parameter(Mandatory = $true)]
    [string]$MsiPath
)

$ErrorActionPreference = "Stop"
$resolved = (Resolve-Path $MsiPath).Path
Write-Host "Verifying MSI database: $resolved"

$installer = New-Object -ComObject WindowsInstaller.Installer
$db = $installer.GetType().InvokeMember("OpenDatabase", "InvokeMethod", $null, $installer, @($resolved, 0))

function Query-MsiTable {
    param([string]$Sql)
    $view = $db.GetType().InvokeMember("OpenView", "InvokeMethod", $null, $db, @($Sql))
    $null = $view.GetType().InvokeMember("Execute", "InvokeMethod", $null, $view, $null)
    while ($true) {
        $rec = $view.GetType().InvokeMember("Fetch", "InvokeMethod", $null, $view, $null)
        if ($null -eq $rec) { break }
        $colCount = [int]$rec.GetType().InvokeMember("FieldCount", "GetProperty", $null, $rec, $null)
        $vals = @()
        for ($i = 1; $i -le $colCount; $i++) {
            $vals += [string]$rec.GetType().InvokeMember("StringData", "GetProperty", $null, $rec, @($i))
        }
        [PSCustomObject]@{ Cols = $vals }
    }
    $null = $view.GetType().InvokeMember("Close", "InvokeMethod", $null, $view, $null)
}

$props = @{}
foreach ($r in (Query-MsiTable 'SELECT `Property`, `Value` FROM `Property`')) {
    $props[$r.Cols[0]] = $r.Cols[1]
}

Write-Host "  ProductName    = $($props['ProductName'])"
Write-Host "  Manufacturer   = $($props['Manufacturer'])"
Write-Host "  ProductVersion = $($props['ProductVersion'])"
Write-Host "  UpgradeCode    = $($props['UpgradeCode'])"
Write-Host "  ARPPRODUCTICON = $($props['ARPPRODUCTICON'])"

if ($props['ProductName'] -ne 'Overture') { throw "Expected ProductName=Overture" }
if ($props['Manufacturer'] -ne 'Overture Project') { throw "Expected Manufacturer=Overture Project" }
if ($props['ProductVersion'] -ne '0.1.1') { throw "Expected ProductVersion=0.1.1, got $($props['ProductVersion'])" }
if ($props['UpgradeCode'] -ne '{9F4E8A71-3D2C-4B85-A619-7C80E5D12F01}') { throw "Unexpected UpgradeCode: $($props['UpgradeCode'])" }
if ($props['ARPPRODUCTICON'] -ne 'ProductICO') { throw "Missing ARPPRODUCTICON" }

$shortcuts = @(Query-MsiTable 'SELECT `Shortcut`, `Name`, `Target` FROM `Shortcut`')
Write-Host "  Shortcuts      = $($shortcuts.Count)"
if ($shortcuts.Count -lt 1) { throw "Expected Start Menu shortcut in Shortcut table" }

$scProps = @(Query-MsiTable 'SELECT `Shortcut_`, `PropertyKey`, `PropVariantValue` FROM `MsiShortcutProperty`')
$aumidFound = $false
foreach ($sp in $scProps) {
    Write-Host "  ShortcutProp   = $($sp.Cols[0]): $($sp.Cols[1]) = $($sp.Cols[2])"
    if ($sp.Cols[1] -eq 'System.AppUserModel.ID' -and $sp.Cols[2] -eq 'Overture.WorshipProjection.App.1') {
        $aumidFound = $true
    }
}
if (-not $aumidFound) { throw "Missing System.AppUserModel.ID = Overture.WorshipProjection.App.1 in MsiShortcutProperty table" }

$files = @(Query-MsiTable 'SELECT `File`, `FileName` FROM `File`')
$fileNames = $files | ForEach-Object { ($_.Cols[1] -split '\|')[-1] }
Write-Host "  Packaged files = $($fileNames -join ', ')"

foreach ($expected in @('overture.exe', 'libmpv-2.dll', 'd3dcompiler_47.dll', 'mpv.conf', 'overture.ico', 'overture-512.png')) {
    if ($fileNames -notcontains $expected) {
        throw "Expected packaged file missing from MSI: $expected"
    }
}

$upgrades = @(Query-MsiTable 'SELECT `UpgradeCode`, `Attributes` FROM `Upgrade`')
if ($upgrades.Count -lt 1) { throw "Missing MajorUpgrade entries in Upgrade table" }

[System.Runtime.InteropServices.Marshal]::ReleaseComObject($db) | Out-Null
[System.Runtime.InteropServices.Marshal]::ReleaseComObject($installer) | Out-Null
[GC]::Collect()
[GC]::WaitForPendingFinalizers()

Write-Host "MSI_VERIFICATION_OK"
