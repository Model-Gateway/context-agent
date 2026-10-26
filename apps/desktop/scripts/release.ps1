# Builds the Windows installers of a Context release, signed with Azure
# Artifact Signing, and copies them to dist\ at the repository root:
# Context_<version>_x64-setup.exe (per-user NSIS) and
# Context_<version>_x64_en-US.msi (per-machine). Tauri signs the app, its
# ctx.exe sidecar and the uninstaller before packing them, then signs both
# installers; the script checks every one of those signatures afterwards. The
# Windows counterpart of apps/macos/scripts/release.sh. The release workflow
# (.github/workflows/release-windows.yml) runs it on windows-latest.
#
# One-time setup (Azure portal):
#   1. An Artifact Signing account, a completed identity validation and a
#      Public Trust certificate profile.
#   2. The "Artifact Signing Certificate Profile Signer" role on the account for
#      whoever signs: you (for local runs) or the CI app registration.
#   3. On the signing machine: SignTool from Windows SDK 10.0.22621.755 or later,
#      the .NET 8 runtime and the Azure CLI, signed in with `az login`. Signing
#      authenticates with the Azure CLI's login and nothing else.
#
# Usage: powershell -ExecutionPolicy Bypass -File apps\desktop\scripts\release.ps1 [-Version <version>]
# Environment: ARTIFACT_SIGNING_ENDPOINT (e.g. https://eus.codesigning.azure.net),
#              ARTIFACT_SIGNING_ACCOUNT, ARTIFACT_SIGNING_PROFILE (required unless SKIP_SIGN=1),
#              SKIP_SIGN=1 to build unsigned installers,
#              SIGNTOOL (default: the newest signtool.exe in the Windows 10 SDK),
#              PUBLISH=1 to upload the installers to the GitHub release v<version>, which
#              must already exist (release.sh PUBLISH=1 creates it) and point at this
#              checkout's commit; needs a clean checkout,
#              RELEASE_REPO (default: Model-Gateway/context-agent).
param([string]$Version)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$here = Resolve-Path (Join-Path $PSScriptRoot "..")
$root = Resolve-Path (Join-Path $here "..\..")
if (-not $Version) {
  $Version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
}
$slug = if ($env:RELEASE_REPO) { $env:RELEASE_REPO } else { "Model-Gateway/context-agent" }
$sign = $env:SKIP_SIGN -ne "1"
$publish = $env:PUBLISH -eq "1"

function Assert-Exit([string]$What) {
  if ($LASTEXITCODE -ne 0) { throw "release.ps1: $What failed (exit $LASTEXITCODE)" }
}

if ($publish) {
  if (git -C $root status --porcelain) {
    throw "release.ps1: PUBLISH=1 needs a clean checkout of the commit being released (try a git worktree)"
  }
  $released = gh api "repos/$slug/commits/v$Version" --jq .sha
  Assert-Exit "looking up the release tag v$Version on $slug (create the release first: release.sh PUBLISH=1)"
  $head = git -C $root rev-parse HEAD
  if ($released -ne $head) {
    throw "release.ps1: v$Version on $slug is $released, but this checkout is $head"
  }
}
if ($sign) {
  foreach ($name in "ARTIFACT_SIGNING_ENDPOINT", "ARTIFACT_SIGNING_ACCOUNT", "ARTIFACT_SIGNING_PROFILE") {
    if (-not [Environment]::GetEnvironmentVariable($name)) {
      throw "release.ps1: set $name (or SKIP_SIGN=1 for unsigned installers)"
    }
  }
}

$work = Join-Path $here "src-tauri\target\release-signing"
$dist = Join-Path $root "dist"
New-Item -ItemType Directory -Force -Path $work, $dist | Out-Null

Write-Host "==> building ctx $Version"
& (Join-Path $PSScriptRoot "embed-ctx.ps1")
Assert-Exit "building ctx"
$ctxVersion = & (Join-Path $root "target\release\ctx.exe") --version
if ($ctxVersion -ne "ctx $Version") {
  throw "release.ps1: ctx reports '$ctxVersion', not 'ctx $Version' (bump the workspace version in Cargo.toml)"
}

# Merged over tauri.conf.json: the release version (installer names, the
# exe's version resource, the About screen) and, when signing, the command
# Tauri runs on every binary and installer it produces.
$config = @{ version = $Version }
if ($sign) {
  Write-Host "==> preparing Artifact Signing ($env:ARTIFACT_SIGNING_ACCOUNT/$env:ARTIFACT_SIGNING_PROFILE)"
  $signtool = $env:SIGNTOOL
  if (-not $signtool) {
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\signtool.exe" -ErrorAction SilentlyContinue |
      Sort-Object { [version]$_.Directory.Parent.Name } | Select-Object -Last 1 -ExpandProperty FullName
  }
  if (-not $signtool -or -not (Test-Path $signtool)) {
    throw "release.ps1: no signtool.exe (install the Windows SDK or set SIGNTOOL)"
  }
  Write-Host "    signtool: $signtool"

  # The SignTool plugin that signs with the account's certificate. The package
  # is pinned by hash; bump both together.
  $clientVersion = "1.0.146"
  $clientSha256 = "7761E5D8F31E54B764DA21D33CD8B0A2940E1B05526C7DFCB5E94B0431426F70"
  $client = Join-Path $work "artifact-signing-client-$clientVersion"
  $dlib = Join-Path $client "bin\x64\Azure.CodeSigning.Dlib.dll"
  if (-not (Test-Path $dlib)) {
    $package = Join-Path $work "artifact-signing-client-$clientVersion.zip"
    Invoke-WebRequest -UseBasicParsing -Uri "https://www.nuget.org/api/v2/package/Microsoft.ArtifactSigning.Client/$clientVersion" -OutFile $package
    $hash = (Get-FileHash -Algorithm SHA256 -Path $package).Hash
    if ($hash -ne $clientSha256) { throw "release.ps1: Microsoft.ArtifactSigning.Client $clientVersion has SHA-256 $hash, expected $clientSha256" }
    Expand-Archive -Path $package -DestinationPath $client -Force
  }

  # Sign with the Azure CLI's login (az login here, azure/login in CI) and skip
  # the credentials DefaultAzureCredential would otherwise try first.
  $metadata = Join-Path $work "metadata.json"
  $metadataJson = @{
    Endpoint               = $env:ARTIFACT_SIGNING_ENDPOINT
    CodeSigningAccountName = $env:ARTIFACT_SIGNING_ACCOUNT
    CertificateProfileName = $env:ARTIFACT_SIGNING_PROFILE
    ExcludeCredentials     = @(
      "EnvironmentCredential", "WorkloadIdentityCredential", "ManagedIdentityCredential",
      "SharedTokenCacheCredential", "VisualStudioCredential", "VisualStudioCodeCredential",
      "AzurePowerShellCredential", "AzureDeveloperCliCredential", "InteractiveBrowserCredential"
    )
  } | ConvertTo-Json
  [IO.File]::WriteAllText($metadata, $metadataJson)

  $config.bundle = @{ windows = @{ signCommand = @{
    cmd  = $signtool
    args = @("sign", "/v", "/fd", "SHA256", "/tr", "http://timestamp.acs.microsoft.com", "/td", "SHA256",
      "/dlib", $dlib, "/dmdf", $metadata, "/d", "Context", "%1")
  } } }
}
$releaseConfig = Join-Path $work "tauri.release.conf.json"
# WriteAllText writes UTF-8 without a BOM, which the Tauri CLI's JSON parser needs.
[IO.File]::WriteAllText($releaseConfig, ($config | ConvertTo-Json -Depth 6))

Write-Host "==> building the installers"
Push-Location $here
try {
  npm ci
  Assert-Exit "npm ci"
  npm run tauri build -- --config src-tauri/tauri.bundle.conf.json --config $releaseConfig
  Assert-Exit "tauri build"
} finally { Pop-Location }

$bundle = Join-Path $here "src-tauri\target\release\bundle"
$setup = Join-Path $bundle "nsis\Context_${Version}_x64-setup.exe"
$msi = Join-Path $bundle "msi\Context_${Version}_x64_en-US.msi"
foreach ($installer in $setup, $msi) {
  if (-not (Test-Path $installer)) { throw "release.ps1: the build did not produce $installer" }
}

if ($sign) {
  Write-Host "==> checking signatures"
  function Assert-Signed([string]$Path) {
    $signature = Get-AuthenticodeSignature -FilePath $Path
    if ($signature.Status -ne "Valid") {
      throw "release.ps1: $Path is not validly signed ($($signature.Status): $($signature.StatusMessage))"
    }
    if (-not $signature.TimeStamperCertificate) { throw "release.ps1: $Path has no timestamp" }
    Write-Host "    ok  $(Split-Path -Leaf $Path)  ($($signature.SignerCertificate.Subject))"
  }
  Assert-Signed $setup
  Assert-Signed $msi

  # What the .msi installs: every executable in it must be signed. An
  # administrative install only unpacks it (and may need an elevated shell).
  $msiFiles = Join-Path $work "msi-contents"
  Remove-Item -Recurse -Force $msiFiles -ErrorAction SilentlyContinue
  $unpack = Start-Process msiexec.exe -ArgumentList "/a", "`"$msi`"", "/qn", "TARGETDIR=`"$msiFiles`"" -Wait -PassThru
  if ($unpack.ExitCode -eq 0) {
    $executables = @(Get-ChildItem -Path $msiFiles -Recurse -Include *.exe, *.dll)
    foreach ($name in "Context.exe", "ctx.exe") {
      if (-not ($executables | Where-Object Name -eq $name)) { throw "release.ps1: the msi has no $name" }
    }
    $executables | ForEach-Object { Assert-Signed $_.FullName }
  } else {
    Write-Warning "could not unpack the msi (msiexec exit $($unpack.ExitCode)); its contents were not checked"
  }

  # What the setup .exe installs (the same signed files as the .msi), when
  # 7-Zip, which reads NSIS archives, is around: the app and ctx.exe must be
  # signed, the rest is listed.
  if (Get-Command 7z -ErrorAction SilentlyContinue) {
    $setupFiles = Join-Path $work "setup-contents"
    Remove-Item -Recurse -Force $setupFiles -ErrorAction SilentlyContinue
    7z x $setup "-o$setupFiles" -y | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Warning "7-Zip could not fully unpack the setup exe (exit $LASTEXITCODE)" }
    $found = 0
    foreach ($file in Get-ChildItem -Path $setupFiles -Recurse -Include *.exe, *.dll) {
      if ("Context.exe", "ctx.exe" -contains $file.Name) {
        Assert-Signed $file.FullName
        $found++
      } else {
        Write-Host "    --  $($file.FullName.Replace("$setupFiles\", ''))  ($((Get-AuthenticodeSignature -FilePath $file.FullName).Status))"
      }
    }
    if ($found -lt 2) { Write-Warning "7-Zip did not find both Context.exe and ctx.exe in the setup exe" }
  }
}

Copy-Item -Path $setup, $msi -Destination $dist -Force
$outputs = @(
  (Join-Path $dist (Split-Path -Leaf $setup)),
  (Join-Path $dist (Split-Path -Leaf $msi))
)

if ($publish) {
  Write-Host "==> uploading to v$Version on $slug"
  gh release upload "v$Version" $outputs --repo $slug --clobber
  Assert-Exit "gh release upload"
}

Get-FileHash -Algorithm SHA256 -Path $outputs | ForEach-Object { "{0}  {1}" -f $_.Hash.ToLower(), (Split-Path -Leaf $_.Path) }
Write-Host "==> done: $dist"
