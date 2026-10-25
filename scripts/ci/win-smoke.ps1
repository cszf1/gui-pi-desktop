# Windows CI smoke test of the INSTALLED app (the runner has an interactive desktop):
#   install the NSIS package silently, start the installed exe, read its self-check and bundle-integrity lines,
#   take screenshots; the test-hooks build also shows the four person popups through the real ask-a-person IPC
#   (--demo-popups), screenshots each one and checks every answer (one of them is left to time out = no answer).
#   Finally the bundled gui-gate.exe runs gui-gate's fake_driver end-to-end suite and the app is uninstalled.
# usage: pwsh scripts/ci/win-smoke.ps1 -Setup <setup.exe> -Out <dir> -Variant normal|testhooks -GateSrc <gui-gate checkout>
param([string]$Setup, [string]$Out, [string]$Variant, [string]$GateSrc)
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $Out | Out-Null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$fails = @()

function Shot([string]$name) {
  $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
  $p = Join-Path $Out "$name.png"
  $bmp.Save($p, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  Write-Host "screenshot $p"
}

function WaitFor([string]$file, [string]$pattern, [int]$secs) {
  $t = [DateTime]::Now.AddSeconds($secs)
  while ([DateTime]::Now -lt $t) {
    if ((Test-Path $file) -and (Select-String -Path $file -Pattern $pattern -SimpleMatch -Quiet)) { return $true }
    Start-Sleep -Milliseconds 250
  }
  return $false
}

# --- install (per-user NSIS, silent) ---
$p = Start-Process -FilePath $Setup -ArgumentList "/S" -Wait -PassThru
Write-Host "installer exit $($p.ExitCode)"
if ($p.ExitCode -ne 0) { throw "installer failed" }
$exe = Get-ChildItem -Path $env:LOCALAPPDATA -Filter gui-pi-desktop.exe -Recurse -Depth 2 -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $exe) { throw "installed gui-pi-desktop.exe not found under $env:LOCALAPPDATA" }
$dir = $exe.DirectoryName
Write-Host "installed: $($exe.FullName)"
"installed: $($exe.FullName)" | Out-File (Join-Path $Out "install.txt")
foreach ($f in @("resources\node\bin\node.exe", "resources\gui-gate.exe", "resources\cua-driver\cua-driver.exe", "resources\SHA256SUMS")) {
  if (-not (Test-Path (Join-Path $dir $f))) { $fails += "missing after install: $f" }
}

$so = Join-Path $Out "app.stdout.txt"; $se = Join-Path $Out "app.stderr.txt"
if ($Variant -eq "testhooks") {
  $rules = Join-Path $Out "auto.json"
  '{"delay_ms": 7000, "approve": ["yes"], "answer": ["Shuffle labels"], "takeover": ["yes"], "ask_help": ["timeout"]}' | Out-File -Encoding ascii $rules
  $env:GUI_PI_HUMAN_AUTO = $rules
  $env:GUI_PI_DEMO_LEAD_MS = "5000"
  $app = Start-Process -FilePath $exe.FullName -ArgumentList "--demo-popups", "15", "--show-settings" -PassThru `
           -RedirectStandardOutput $so -RedirectStandardError $se
  if (WaitFor $se "[self-check] bundle" 120) { Start-Sleep -Seconds 3; Shot "01-main-and-settings" } else { $fails += "no self-check output" }
  $n = 0
  foreach ($t in @("approve", "clarify", "takeover", "help")) {
    $n++
    if (WaitFor $so "`"type`":`"$t`"" 90) { Start-Sleep -Milliseconds 2500; Shot ("0{0}-popup-{1}" -f ($n + 1), $t) } else { $fails += "popup $t never asked" }
  }
  if (-not $app.WaitForExit(90000)) { $fails += "demo did not finish"; Stop-Process -Id $app.Id -Force }
  Write-Host "app exit $($app.ExitCode)"
  $replies = Get-Content $so | Where-Object { $_ -like '*"demo_reply"*' } | ForEach-Object { $_ | ConvertFrom-Json }
  $want = @{ approve = 'accept|allow'; answer = 'accept|Shuffle labels'; takeover = 'accept|True'; ask_help = 'decline|timed_out' }
  foreach ($k in $want.Keys) {
    $r = $replies | Where-Object { $_.gate_kind -eq $k } | Select-Object -First 1
    if (-not $r) { $fails += "no reply for $k"; continue }
    $parts = $want[$k].Split("|")
    $flat = ($r.reply | ConvertTo-Json -Compress -Depth 5)
    Write-Host "reply ${k}: $flat"
    if ($r.reply.action -ne $parts[0] -or -not $flat.Contains($parts[1].ToLower()) -and -not $flat.Contains($parts[1])) { $fails += "reply for ${k}: $flat" }
  }
  Remove-Item Env:GUI_PI_HUMAN_AUTO
} else {
  # the normal build: plain start (screenshot), then --run with no settings: the self-check runs and Start is refused
  $app = Start-Process -FilePath $exe.FullName -PassThru
  Start-Sleep -Seconds 15
  Shot "01-main-window"
  Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 2
  $app = Start-Process -FilePath $exe.FullName -ArgumentList "--run", "smoke test" -PassThru -RedirectStandardOutput $so -RedirectStandardError $se
  if (-not $app.WaitForExit(120000)) { $fails += "--run did not end"; Stop-Process -Id $app.Id -Force }
  Write-Host "app --run exit $($app.ExitCode) (3 = refused to start: no models configured)"
  if ($app.ExitCode -ne 3) { $fails += "--run exit $($app.ExitCode), expected 3" }
}
Write-Host "--- stderr"; Get-Content $se | Select-Object -First 60 | ForEach-Object { Write-Host $_ }
Write-Host "--- stdout"; Get-Content $so | Select-Object -First 40 | ForEach-Object { Write-Host $_ }
$err = Get-Content $se -Raw
if ($err -notmatch "bundled files match the release's SHA256SUMS") { $fails += "bundle integrity not confirmed" }
foreach ($id in @("windows", "webview2", "uia", "keychain")) {
  $line = (Get-Content $se | Where-Object { $_ -like "[self-check] ${id}:*" } | Select-Object -First 1)
  if (-not $line) { $fails += "self-check $id missing" } elseif ($line -like "*: fail -*") { $fails += "self-check failed: $line" }
}

# --- the bundled gate against gui-gate's fake driver (every gate return action) ---
if ($Variant -eq "normal") {
  $env:GUI_GATE_BIN = Join-Path $dir "resources\gui-gate.exe"
  $env:GUI_GATE_NO_UPDATE_CHECK = "1"
  Push-Location $GateSrc
  python -m unittest tests.test_actions_e2e 2>&1 | Tee-Object -FilePath (Join-Path $Out "bundled-gate-e2e.txt")
  if ($LASTEXITCODE -ne 0) { $fails += "bundled gate e2e failed" }
  Pop-Location
}

# --- uninstall ---
$un = Get-ChildItem -Path $dir -Filter "uninstall*.exe" | Select-Object -First 1
if ($un) {
  $p = Start-Process -FilePath $un.FullName -ArgumentList "/S" -Wait -PassThru
  Start-Sleep -Seconds 5
  Write-Host "uninstaller exit $($p.ExitCode); exe still there: $(Test-Path $exe.FullName)"
  if (Test-Path $exe.FullName) { $fails += "uninstall left the exe" }
} else { $fails += "no uninstaller" }

if ($fails.Count) { Write-Host "FAILURES:"; $fails | ForEach-Object { Write-Host "  $_" }; $fails | Out-File (Join-Path $Out "failures.txt"); exit 1 }
Write-Host "smoke ok"
