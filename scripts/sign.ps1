<#
.SYNOPSIS
  Signing hook called by Tauri (bundle.windows.signCommand) for every binary it produces.
.DESCRIPTION
  Route-agnostic: the actual command comes from MARKY_SIGN_COMMAND, with {file} as the
  placeholder, so an OV certificate (signtool), Certum SimplySign, SSL.com CodeSignTool or
  Azure Artifact Signing all plug in without code changes. Examples:

    signtool sign /fd sha256 /tr http://time.certum.pl /td sha256 /sha1 <THUMBPRINT> "{file}"
    CodeSignTool.bat sign -credential_id=... -input_file_path="{file}" -override

  Fails fast when the variable is missing, so a "signed" build can never ship unsigned.
.EXAMPLE
  ./scripts/build-signed.ps1
#>
param([Parameter(Mandatory)][string]$File)
$ErrorActionPreference = 'Stop'

$template = $env:MARKY_SIGN_COMMAND
if (-not $template) { throw 'MARKY_SIGN_COMMAND is not set; refusing to produce an unsigned "signed" build.' }
if (-not $template.Contains('{file}')) { throw 'MARKY_SIGN_COMMAND must contain the {file} placeholder.' }
if (-not (Test-Path $File)) { throw "File to sign not found: $File" }

$command = $template.Replace('{file}', $File)
Write-Host "Signing $File"
cmd /c $command
if ($LASTEXITCODE -ne 0) { throw "Signing failed ($LASTEXITCODE) for $File" }
