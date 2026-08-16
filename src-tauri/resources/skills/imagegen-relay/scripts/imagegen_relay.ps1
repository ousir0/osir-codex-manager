param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet("generate", "edit")]
    [string]$Mode,

    [Parameter(Mandatory = $true)]
    [string]$Prompt,

    [string[]]$InputPath
)

$ErrorActionPreference = "Stop"

function Fail([string]$Message) {
    Write-Error $Message
    exit 1
}

function Get-ImageSource($Result) {
    $roots = @($Result)
    $resultProperty = $Result.PSObject.Properties["result"]
    if ($resultProperty -and $null -ne $resultProperty.Value) {
        $roots += @($resultProperty.Value)
    }
    $items = @()
    foreach ($root in $roots) {
        $items += @($root)
        foreach ($containerName in @("data", "images", "output")) {
            $property = $root.PSObject.Properties[$containerName]
            if ($property -and $null -ne $property.Value) {
                $items += @($property.Value)
            }
        }
    }
    foreach ($item in $items) {
        if ($item -is [string] -and $item.Trim()) {
            return [PSCustomObject]@{ Kind = "auto"; Value = $item.Trim() }
        }
        if ($null -eq $item) { continue }
        foreach ($name in @("b64_json", "base64", "image_base64", "image")) {
            $property = $item.PSObject.Properties[$name]
            if ($property -and $property.Value -is [string] -and $property.Value.Trim()) {
                return [PSCustomObject]@{ Kind = "base64"; Value = $property.Value.Trim() }
            }
        }
        foreach ($name in @("url", "image_url")) {
            $property = $item.PSObject.Properties[$name]
            if (-not $property) { continue }
            $value = $property.Value
            if ($null -ne $value -and $value -isnot [string]) {
                $nestedUrl = $value.PSObject.Properties["url"]
                if ($nestedUrl) { $value = $nestedUrl.Value }
            }
            if ($value -is [string] -and $value.Trim()) {
                return [PSCustomObject]@{ Kind = "url"; Value = $value.Trim() }
            }
        }
    }
    Fail "Image API response did not contain an image payload"
}

function Get-ImageExtension([string]$Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -ge 8 -and [BitConverter]::ToString($bytes, 0, 8) -eq "89-50-4E-47-0D-0A-1A-0A") { return ".png" }
    if ($bytes.Length -ge 3 -and [BitConverter]::ToString($bytes, 0, 3) -eq "FF-D8-FF") { return ".jpg" }
    if ($bytes.Length -ge 6) {
        $head = [Text.Encoding]::ASCII.GetString($bytes, 0, 6)
        if ($head -eq "GIF87a" -or $head -eq "GIF89a") { return ".gif" }
    }
    if ($bytes.Length -ge 12) {
        if ([Text.Encoding]::ASCII.GetString($bytes, 0, 4) -eq "RIFF" -and [Text.Encoding]::ASCII.GetString($bytes, 8, 4) -eq "WEBP") { return ".webp" }
        $brand = [Text.Encoding]::ASCII.GetString($bytes, 4, 8)
        if ($brand -eq "ftypavif" -or $brand -eq "ftypavis") { return ".avif" }
    }
    Fail "Image API payload is not a supported image file"
}

$configPath = Join-Path $env:USERPROFILE ".codex\imagegen-relay.json"
try {
    $config = Get-Content -LiteralPath $configPath -Raw -Encoding UTF8 | ConvertFrom-Json
} catch {
    Fail "Image API is not configured: $configPath"
}

$baseUrl = ([string]$config.base_url).Trim().TrimEnd("/")
$apiKey = ([string]$config.api_key).Trim()
$model = ([string]$config.model).Trim()
if (-not $model) { $model = "gpt-image-2" }
if (-not $baseUrl -or -not $apiKey) { Fail "Image API configuration is incomplete" }
if (-not (Get-Command curl.exe -ErrorAction SilentlyContinue)) { Fail "curl.exe was not found" }

$responsePath = [IO.Path]::GetTempFileName()
$requestPath = $null
$imageTempPath = $null
try {
    $curlArgs = @(
        "--silent",
        "--show-error",
        "--fail",
        "--max-time", "180",
        "-H", "Authorization: Bearer $apiKey"
    )

    if ($Mode -eq "edit") {
        if (-not $InputPath -or $InputPath.Count -eq 0) {
            Fail "edit requires at least one input image"
        }
        $endpoint = "$baseUrl/images/edits"
        $fieldName = if ($InputPath.Count -eq 1) { "image" } else { "image[]" }
        $curlArgs += @("-F", "model=$model", "-F", "prompt=$Prompt")
        foreach ($candidate in $InputPath) {
            $resolved = (Resolve-Path -LiteralPath $candidate -ErrorAction Stop).Path
            $curlArgs += @("-F", "$fieldName=@$resolved")
        }
    } else {
        $endpoint = "$baseUrl/images/generations"
        $requestPath = [IO.Path]::GetTempFileName()
        $payload = @{ model = $model; prompt = $Prompt } | ConvertTo-Json -Compress
        [IO.File]::WriteAllText($requestPath, $payload, [Text.UTF8Encoding]::new($false))
        $curlArgs += @("-H", "Content-Type: application/json", "--data-binary", "@$requestPath")
    }

    $curlArgs += @("-o", $responsePath, $endpoint)
    & curl.exe @curlArgs
    if ($LASTEXITCODE -ne 0) {
        $body = Get-Content -LiteralPath $responsePath -Raw -ErrorAction SilentlyContinue
        Fail "Image API request failed (curl $LASTEXITCODE): $body"
    }

    try {
        $result = Get-Content -LiteralPath $responsePath -Raw -Encoding UTF8 | ConvertFrom-Json
    } catch {
        Fail "Image API returned invalid JSON"
    }

    $outputDir = Join-Path $env:USERPROFILE ".codex\generated_images\relay"
    [IO.Directory]::CreateDirectory($outputDir) | Out-Null
    $stem = "image-{0}-{1}" -f (Get-Date -Format "yyyyMMdd-HHmmss"), [guid]::NewGuid().ToString("N").Substring(0, 8)
    $imageTempPath = Join-Path $outputDir "$stem.tmp"
    $source = Get-ImageSource $result
    $sourceValue = [string]$source.Value
    if ($sourceValue -match "(?s)^data:image/[^;,]+;base64,(.+)$") {
        $source.Kind = "base64"
        $sourceValue = $Matches[1]
    } elseif ($source.Kind -eq "auto") {
        $source.Kind = if ($sourceValue -match "^https?://") { "url" } else { "base64" }
    }
    if ($source.Kind -eq "url") {
        try { $imageUri = [Uri]$sourceValue } catch { Fail "Image API returned an invalid image URL" }
        if (-not $imageUri.IsAbsoluteUri -or $imageUri.Scheme -notin @("http", "https")) {
            Fail "Image API returned an unsupported image URL"
        }
        $downloadArgs = @("--silent", "--show-error", "--fail", "--location", "--max-time", "180")
        try {
            $apiOrigin = ([Uri]$baseUrl).GetLeftPart([UriPartial]::Authority)
            $imageOrigin = $imageUri.GetLeftPart([UriPartial]::Authority)
            if ($apiOrigin -eq $imageOrigin) { $downloadArgs += @("-H", "Authorization: Bearer $apiKey") }
        } catch {
            Fail "Image API base URL is invalid"
        }
        $downloadArgs += @("-o", $imageTempPath, $sourceValue)
        & curl.exe @downloadArgs
        if ($LASTEXITCODE -ne 0) { Fail "Image URL download failed (curl $LASTEXITCODE)" }
    } else {
        try {
            $imageBytes = [Convert]::FromBase64String(($sourceValue -replace "\s", ""))
        } catch {
            Fail "Image API returned invalid base64 image data"
        }
        [IO.File]::WriteAllBytes($imageTempPath, $imageBytes)
    }
    if (-not (Test-Path -LiteralPath $imageTempPath) -or (Get-Item -LiteralPath $imageTempPath).Length -eq 0) {
        Fail "Generated image file is empty"
    }
    $extension = Get-ImageExtension $imageTempPath
    $outputPath = Join-Path $outputDir "$stem$extension"
    Move-Item -LiteralPath $imageTempPath -Destination $outputPath -Force
    $imageTempPath = $null

    $previewPath = $outputPath.Replace("\", "/")
    Write-Output "![preview]($previewPath)"
    Write-Output $previewPath
} finally {
    Remove-Item -LiteralPath $responsePath -Force -ErrorAction SilentlyContinue
    if ($requestPath) {
        Remove-Item -LiteralPath $requestPath -Force -ErrorAction SilentlyContinue
    }
    if ($imageTempPath) {
        Remove-Item -LiteralPath $imageTempPath -Force -ErrorAction SilentlyContinue
    }
}
