param([string]$RepositoryRoot = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)))
$ErrorActionPreference = 'Stop'
$engine = Join-Path $RepositoryRoot 'windows/engine/windows-engine.ps1'
$tokens=$null; $parseErrors=$null
$ast=[System.Management.Automation.Language.Parser]::ParseFile($engine,[ref]$tokens,[ref]$parseErrors)
if ($parseErrors.Count -ne 0) { throw 'Original engine parsing failed' }
# Load original top-level function definitions only. Never execute the production entrypoint.
foreach ($statement in $ast.EndBlock.Statements) {
    if ($statement -is [System.Management.Automation.Language.FunctionDefinitionAst]) {
        Invoke-Expression $statement.Extent.Text
    }
}
function Assert-Equal($Expected,$Actual,[string]$Label) {
    if ($Expected -cne $Actual) { throw ('Assertion failed: '+$Label) }
}
$work=Join-Path ([System.IO.Path]::GetTempPath()) ('tcm-characterization-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
try {
    foreach ($pair in @(@('movie','Movie'),@('tvshow','Series'),@('season','Season'),@('episodedetails','Episode'))) {
        $path=Join-Path $work ($pair[0]+'.nfo')
        $text=('<'+$pair[0]+'>`r`n<title>测试</title><uniqueid type="imdb">tt0064757</uniqueid><tag>外部</tag><tag>生成</tag><technicalspecs source="IMDb" imdbid="tt0064757"><section name="Camera"><item>ARRI</item><item>ARRI</item></section><generatedtags owner="IMDb Tech Manager" engine="local"><tag>生成</tag></generatedtags></technicalspecs></'+$pair[0]+'>').Replace('`r`n',"`r`n")
        [System.IO.File]::WriteAllText($path,$text,[System.Text.UTF8Encoding]::new($true))
        $before=Get-FileHash -LiteralPath $path -Algorithm SHA256
        $time=(Get-Item -LiteralPath $path).LastWriteTimeUtc.Ticks
        $obj=Get-TechObject -NfoPath $path
        Assert-Equal $pair[1] $obj.type 'media type'
        Assert-Equal 'tt0064757' $obj.imdb 'IMDb'
        Assert-Equal 1 @($obj.specs['Camera']).Count 'deduplicate direct sections'
        Assert-Equal 'external' $obj.tags[0].ownership 'external ownership'
        Assert-Equal 'generated' $obj.tags[1].ownership 'manifest ownership'
        Assert-Equal $before.Hash (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash 'NFO bytes unchanged'
        Assert-Equal $time (Get-Item -LiteralPath $path).LastWriteTimeUtc.Ticks 'NFO modification time unchanged'
    }
    $broken=Join-Path $work 'broken.nfo'
    [System.IO.File]::WriteAllText($broken,'<movie>')
    $raised=$false
    try { Get-TechObject -NfoPath $broken | Out-Null } catch { $raised=$true }
    Assert-Equal $true $raised 'malformed XML must raise'
    Assert-Equal '<movie>' ([System.IO.File]::ReadAllText($broken)) 'malformed XML not repaired'
    $destination=Join-Path $work 'atomic.json'
    Save-BytesTransactional -Path $destination -Bytes ([System.Text.Encoding]::UTF8.GetBytes('first'))
    Save-BytesTransactional -Path $destination -Bytes ([System.Text.Encoding]::UTF8.GetBytes('second'))
    Assert-Equal 'second' ([System.IO.File]::ReadAllText($destination)) 'replace readback'
    $lock=[System.IO.File]::Open($destination,[System.IO.FileMode]::Open,[System.IO.FileAccess]::Read,[System.IO.FileShare]::None)
    $raised=$false
    try { Save-BytesTransactional -Path $destination -Bytes ([System.Text.Encoding]::UTF8.GetBytes('third')) } catch { $raised=$true } finally { $lock.Dispose() }
    Assert-Equal $true $raised 'locked destination must fail'
    Assert-Equal 'second' ([System.IO.File]::ReadAllText($destination)) 'failed replacement preserves prior bytes'
    Write-Output 'PASS original PowerShell functions: four media types, ownership, read-only bytes/mtime, parse failure, transactional replace and lock failure'
} finally { Remove-Item -LiteralPath $work -Recurse -Force }
