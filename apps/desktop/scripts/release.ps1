# Builds the Windows installers of a Context release, signed with Azure
# Artifact Signing, and copies them to dist\ at the repository root:
# Context_<version>_x64-setup.exe (per-user NSIS) and
# Context_<version>_x64_en-US.msi (per-machine). Tauri signs the app, its
# ctx.exe sidecar and the uninstaller before packing them, then signs both
# installers; the script checks every one of those signatures afterwards.
# With the updater key (TAURI_SIGNING_PRIVATE_KEY, required when signing) it
# also writes the feed installed copies update from (src-tauri/src/updater.rs):
# a minisign signature of each finished installer (<installer>.sig, Tauri's
# createUpdaterArtifacts) and dist\latest.json, which names both installers.
# The app reads https://github.com/<repo>/releases/latest/download/latest.json,
# so every GitHub release must carry its latest.json, as it carries the macOS
# appcast.xml. The Windows counterpart of apps/macos/scripts/release.sh. The
# release workflow (.github/workflows/release-windows.yml) runs it on
# windows-latest.
#
# One-time setup (Azure portal):
#   1. An Artifact Signing account, a completed identity validation and a
#      Public Trust certificate profile.
#   2. The "Artifact Signing Certificate Profile Signer" role on the account for
#      whoever signs: you (for local runs) or the CI app registration.
#   3. On the signing machine: SignTool from Windows SDK 10.0.22621.755 or later,
#      the .NET 8 runtime and the Azure CLI, signed in with `az login`. Signing
#      authenticates with the Azure CLI's login and nothing else.
#   4. The updater key pair (`npx tauri signer generate -w <file>` in
#      apps/desktop). Its public half is plugins.updater.pubkey in
#      src-tauri/tauri.conf.json; the private key and its password are the
#      windows-release environment's secrets TAURI_SIGNING_PRIVATE_KEY and
#      TAURI_SIGNING_PRIVATE_KEY_PASSWORD. Without the private key no installed
#      copy accepts an update.
#
# Usage: powershell -ExecutionPolicy Bypass -File apps\desktop\scripts\release.ps1 [-Version <version>]
# Environment: ARTIFACT_SIGNING_ENDPOINT (e.g. https://eus.codesigning.azure.net),
#              ARTIFACT_SIGNING_ACCOUNT, ARTIFACT_SIGNING_PROFILE (required unless SKIP_SIGN=1),
#              SKIP_SIGN=1 to build unsigned installers,
#              SIGNTOOL (default: the newest signtool.exe in the Windows 10 SDK),
#              TAURI_SIGNING_PRIVATE_KEY (the updater key, or the path of its file;
#              required unless SKIP_SIGN=1) and TAURI_SIGNING_PRIVATE_KEY_PASSWORD,
#              NOTES=<file.md> release notes for latest.json (default with PUBLISH=1:
#              the GitHub release's notes),
#              PUBLISH=1 to upload the installers, their .sig files and latest.json to
#              the GitHub release v<version>, which must already exist (release.sh
#              PUBLISH=1 creates it) and point at this checkout's commit; needs a
#              clean checkout,
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
$updater = [bool]$env:TAURI_SIGNING_PRIVATE_KEY

function Assert-Exit([string]$What) {
  if ($LASTEXITCODE -ne 0) { throw "release.ps1: $What failed (exit $LASTEXITCODE)" }
}

if ($publish) {
  if (-not $updater) {
    throw "release.ps1: PUBLISH=1 needs TAURI_SIGNING_PRIVATE_KEY: a release without latest.json leaves installed copies without updates"
  }
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
  foreach ($name in "ARTIFACT_SIGNING_ENDPOINT", "ARTIFACT_SIGNING_ACCOUNT", "ARTIFACT_SIGNING_PROFILE", "TAURI_SIGNING_PRIVATE_KEY") {
    if (-not [Environment]::GetEnvironmentVariable($name)) {
      throw "release.ps1: set $name (or SKIP_SIGN=1 for unsigned installers)"
    }
  }
}
if ($env:NOTES -and -not (Test-Path $env:NOTES)) {
  throw "release.ps1: NOTES file not found: $env:NOTES"
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
# exe's version resource, the About screen, what the updater compares), the
# updater signatures when there is a key, and, when signing, the command
# Tauri runs on every binary and installer it produces. Tauri writes the
# updater signatures last, over the Authenticode-signed installers.
$config = @{ version = $Version; bundle = @{ createUpdaterArtifacts = $updater } }
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

  $config.bundle.windows = @{ signCommand = @{
    cmd  = $signtool
    args = @("sign", "/v", "/fd", "SHA256", "/tr", "http://timestamp.acs.microsoft.com", "/td", "SHA256",
      "/dlib", $dlib, "/dmdf", $metadata, "/d", "Context", "%1")
  } }
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
  if ($updater -and -not (Test-Path "$installer.sig")) { throw "release.ps1: the build did not produce $installer.sig (the updater signature)" }
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
  # The app keeps its Cargo binary name (tauri.conf.json sets no mainBinaryName).
  $binaries = "context-desktop.exe", "ctx.exe"

  # What the .msi installs: every executable in it must be signed. An
  # administrative install only unpacks it (and may need an elevated shell).
  $msiFiles = Join-Path $work "msi-contents"
  Remove-Item -Recurse -Force $msiFiles -ErrorAction SilentlyContinue
  $unpack = Start-Process msiexec.exe -ArgumentList "/a", "`"$msi`"", "/qn", "TARGETDIR=`"$msiFiles`"" -Wait -PassThru
  if ($unpack.ExitCode -eq 0) {
    $executables = @(Get-ChildItem -Path $msiFiles -Recurse -Include *.exe, *.dll)
    foreach ($name in $binaries) {
      if (-not ($executables | Where-Object Name -eq $name)) {
        throw "release.ps1: the msi has no $name (it has: $(($executables | ForEach-Object Name) -join ', '))"
      }
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
      if ($binaries -contains $file.Name) {
        Assert-Signed $file.FullName
        $found++
      } else {
        Write-Host "    --  $($file.FullName.Replace("$setupFiles\", ''))  ($((Get-AuthenticodeSignature -FilePath $file.FullName).Status))"
      }
    }
    if ($found -lt 2) { Write-Warning "7-Zip did not find both $($binaries -join ' and ') in the setup exe" }
  }
}

$artifacts = @($setup, $msi)
if ($updater) { $artifacts += @("$setup.sig", "$msi.sig") }
Copy-Item -Path $artifacts -Destination $dist -Force
$outputs = @($artifacts | ForEach-Object { Join-Path $dist (Split-Path -Leaf $_) })

# The update feed: this release only, both installers. An installed copy
# takes the entry of the installer it came from ("-nsis" / "-msi", which the
# bundler marks in the app), and "windows-x86_64" otherwise. The plugin
# checks the downloaded installer against the signature here before running it.
if ($updater) {
  Write-Host "==> writing the update feed"
  $notes = $null
  if ($env:NOTES) {
    $notes = [IO.File]::ReadAllText((Resolve-Path $env:NOTES))
  } elseif ($publish) {
    # gh prints UTF-8; read it as such (the console's code page would mangle the notes).
    $encoding = [Console]::OutputEncoding
    [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
    try {
      $body = gh release view "v$Version" --repo $slug --json body --jq .body
      Assert-Exit "reading the notes of v$Version on $slug"
    } finally { [Console]::OutputEncoding = $encoding }
    $notes = ($body -join "`n").Trim()
  }
  if (-not $notes) { $notes = $null }
  $download = "https://github.com/$slug/releases/download/v$Version"
  function Get-FeedEntry([string]$Installer) {
    [ordered]@{
      signature = ([IO.File]::ReadAllText("$Installer.sig")).Trim()
      url       = "$download/$(Split-Path -Leaf $Installer)"
    }
  }
  $nsis = Get-FeedEntry $setup
  $feed = [ordered]@{
    version   = $Version
    notes     = $notes
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'")
    platforms = [ordered]@{
      "windows-x86_64"      = $nsis
      "windows-x86_64-nsis" = $nsis
      "windows-x86_64-msi"  = Get-FeedEntry $msi
    }
  }
  $latest = Join-Path $dist "latest.json"
  [IO.File]::WriteAllText($latest, ($feed | ConvertTo-Json -Depth 4))
  $outputs += $latest
  Write-Host "    latest.json: $Version, $($feed.platforms.Keys -join ', ')"
}

if ($publish) {
  Write-Host "==> uploading to v$Version on $slug"
  gh release upload "v$Version" $outputs --repo $slug --clobber
  Assert-Exit "gh release upload"
}

Get-FileHash -Algorithm SHA256 -Path $outputs | ForEach-Object { "{0}  {1}" -f $_.Hash.ToLower(), (Split-Path -Leaf $_.Path) }
Write-Host "==> done: $dist"
