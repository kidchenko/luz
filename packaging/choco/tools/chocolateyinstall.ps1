$ErrorActionPreference = 'Stop'

$version = '0.0.1'
$url     = "https://github.com/kidchenko/luz/releases/download/v$version/Luz-Setup-$version.exe"

$packageArgs = @{
  packageName    = 'luz'
  fileType       = 'EXE'
  url64bit       = $url
  # The release workflow rewrites the version and this checksum from the
  # artifact it just built.
  checksum64     = 'REPLACE_WITH_SHA256'
  checksumType64 = 'sha256'
  # Inno Setup silent switches. The autostart and warmer-colours tasks stay
  # at their defaults (on).
  silentArgs     = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-'
  validExitCodes = @(0)
  softwareName   = 'Luz*'
}

Install-ChocolateyPackage @packageArgs
