param(
	[string]$Destination
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path $PSScriptRoot -Parent
$lockPath = Join-Path $repoRoot "godot.lock.json"
$lock = Get-Content -Raw -LiteralPath $lockPath | ConvertFrom-Json
$artifact = $lock.artifacts."windows-x86_64"

if (-not $Destination) {
	$Destination = Join-Path $repoRoot ".tools/godot/$($lock.version)/windows-x86_64"
}

$executablePath = Join-Path $Destination $artifact.executable
$receiptPath = Join-Path $Destination ".gdkit-godot.json"

if ((Test-Path -LiteralPath $executablePath) -and (Test-Path -LiteralPath $receiptPath)) {
	$receipt = Get-Content -Raw -LiteralPath $receiptPath | ConvertFrom-Json
	if ($receipt.sha256 -eq $artifact.sha256) {
		$installedVersion = & $executablePath --version
		if ($LASTEXITCODE -eq 0 -and $installedVersion.StartsWith($artifact.version_prefix)) {
			Write-Output $executablePath
			exit 0
		}
	}
}

if (Test-Path -LiteralPath $Destination) {
	throw "Godot destination already exists but does not match godot.lock.json: $Destination"
}

$destinationParent = Split-Path $Destination -Parent
New-Item -ItemType Directory -Force -Path $destinationParent | Out-Null
$archivePath = Join-Path ([System.IO.Path]::GetTempPath()) "gdkit-godot-$([guid]::NewGuid()).zip"
$stagingPath = Join-Path $destinationParent ".godot-$([guid]::NewGuid())"

try {
	Invoke-WebRequest -Uri $artifact.url -OutFile $archivePath
	$actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
	if ($actualHash -ne $artifact.sha256) {
		throw "Godot archive checksum mismatch: expected $($artifact.sha256), got $actualHash"
	}

	Expand-Archive -LiteralPath $archivePath -DestinationPath $stagingPath
	$stagedExecutable = Join-Path $stagingPath $artifact.executable
	if (-not (Test-Path -LiteralPath $stagedExecutable -PathType Leaf)) {
		throw "Godot archive does not contain $($artifact.executable)"
	}

	$installedVersion = & $stagedExecutable --version
	if ($LASTEXITCODE -ne 0 -or -not $installedVersion.StartsWith($artifact.version_prefix)) {
		throw "Godot version mismatch: expected $($artifact.version_prefix), got $installedVersion"
	}

	New-Item -ItemType File -Path (Join-Path $stagingPath "_sc_") | Out-Null
	@{
		version = $lock.version
		sha256 = $artifact.sha256
		url = $artifact.url
	} | ConvertTo-Json | Set-Content -Encoding utf8 -LiteralPath (Join-Path $stagingPath ".gdkit-godot.json")
	Move-Item -LiteralPath $stagingPath -Destination $Destination
	Write-Output $executablePath
} finally {
	if (Test-Path -LiteralPath $archivePath) {
		Remove-Item -Force -LiteralPath $archivePath
	}
	if (Test-Path -LiteralPath $stagingPath) {
		Remove-Item -Recurse -Force -LiteralPath $stagingPath
	}
}
