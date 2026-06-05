# unix-x installer for Windows (PowerShell)
# Run: iwr https://raw.githubusercontent.com/YOUR_USERNAME/unix-x/main/install.ps1 | iex

$Repo    = "YOUR_USERNAME/unix-x"
$InstallDir = if ($env:UNIX_X_INSTALL_DIR) { $env:UNIX_X_INSTALL_DIR } else { "$env:USERPROFILE\.unix-x\bin" }
$Tools   = @("lx","px","logx","dx","arcx","envx","netx","jsonx","procx","idx","diffx","memx","statx","hashx","termx")
$Arch    = if ([System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -eq "Arm64") { "aarch64" } else { "x86_64" }
$Platform = "windows-$Arch"
$Artifact = "unix-x-$Platform.zip"

Write-Host "unix-x installer"
Write-Host "Platform : $Platform"
Write-Host "Install  : $InstallDir"
Write-Host ""

# Fetch latest release tag
$Release = (Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest").tag_name

if (-not $Release) {
  Write-Error "Could not fetch latest release."
  exit 1
}

$Url = "https://github.com/$Repo/releases/download/$Release/$Artifact"
$Tmp = Join-Path $env:TEMP "unix-x-install"
New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
$ZipPath = Join-Path $Tmp $Artifact

Write-Host "Downloading $Url ..."
Invoke-WebRequest -Uri $Url -OutFile $ZipPath

Expand-Archive -Path $ZipPath -DestinationPath $Tmp -Force

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

foreach ($tool in $Tools) {
  $src = Join-Path $Tmp "$tool.exe"
  $dst = Join-Path $InstallDir "$tool.exe"
  Copy-Item $src $dst -Force
  Write-Host "  v $tool"
}

# Add to PATH if not already there
$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$InstallDir*") {
  [Environment]::SetEnvironmentVariable("PATH", "$UserPath;$InstallDir", "User")
  Write-Host ""
  Write-Host "Added $InstallDir to your PATH."
  Write-Host "Restart your terminal for PATH changes to take effect."
}

Remove-Item $Tmp -Recurse -Force

Write-Host ""
Write-Host "Done. Run 'lx --help' to verify."
