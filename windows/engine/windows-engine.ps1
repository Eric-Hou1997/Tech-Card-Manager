param(
    [switch]$IndexOnly,
    [switch]$CheckOnly,
    [switch]$DiscoverOnly,
    [switch]$ManagedInstall,
    [switch]$RebuildIndexOnly,
    [switch]$RepairWebOnly,
    [switch]$DisableIntegration,
    [string]$RootConfigPath = '',
    [string]$OnlyRoot = '',
    [string]$BackupRootPath = ''
)

$ErrorActionPreference = 'Stop'

$EmbyRoot = Join-Path $env:APPDATA 'Emby-Server'
$DashboardUi = Join-Path $EmbyRoot 'system\dashboard-ui'
$CustomDir = Join-Path $EmbyRoot 'programdata\custom-tech-specs'
$IndexHtml = Join-Path $DashboardUi 'index.html'
$LiveJs = Join-Path $DashboardUi 'technical-specs-card.js'
$MasterJs = Join-Path $CustomDir 'technical-specs-card.js'
$DataFile = Join-Path $DashboardUi 'technical-specs-data.json'
$RuntimeStateFile = Join-Path $DashboardUi 'technical-specs-runtime.json'
$IndexSummaryFile = Join-Path $CustomDir 'manager-index-summary.json'
$StateFile = Join-Path $CustomDir 'manager-state.json'
$CacheFile = Join-Path $CustomDir 'manager-items-cache.json'
$Worker = Join-Path $CustomDir 'technical-specs-worker.ps1'
$Backup = "$IndexHtml.techspecs.original.bak"
$TaskName = 'Emby Technical Specs Web Card'
$RootDiscoveryFile = Join-Path $CustomDir 'manager-root-discovery.json'
$RootStateFile = Join-Path $CustomDir 'manager-root-state.json'
$LibraryDb = Join-Path $EmbyRoot 'programdata\data\library.db'
$XmlErrorFile = Join-Path $CustomDir 'manager-xml-errors.json'
$CatalogFile = Join-Path $CustomDir 'manager-catalog.json'
$ExpectedWebCardVersion = '4.0.2'
$ManagerVersion = '4.0.2'
$ParserCacheVersion = 'tech-card-cache-1'
$PatchBegin = '<!-- IMDbTechManager WebPatch BEGIN -->'
$PatchEnd = '<!-- IMDbTechManager WebPatch END -->'
function Get-WebCardScriptTag {
    # Stable version-only URL: a Manager restart must never change index.html.
    return '<script src="technical-specs-card.js?v=' + $ExpectedWebCardVersion + '" defer></script>'
}
$BundledJs = Join-Path (Split-Path -Parent $PSCommandPath) 'technical-specs-card.js'

function Assert-WebCardVersion {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw ('找不到技术规格前端脚本：' + $Path)
    }
    $text = Read-Utf8Text -Path $Path
    $escaped = [regex]::Escape($ExpectedWebCardVersion)
    if ($text -notmatch ('WEB_CARD_VERSION\s*=\s*["'']' + $escaped + '["'']')) {
        throw ('技术规格前端脚本版本不匹配：期望 ' + $ExpectedWebCardVersion + '，文件：' + $Path)
    }
}

function Sync-ManagerFrontendAsset {
    New-Item -ItemType Directory -Force -Path $CustomDir | Out-Null

    # On first install BundledJs is beside the Manager engine; once copied to the
    # custom directory the worker may see BundledJs == MasterJs. Never fall back
    # to an embedded historical JS blob: the bundled current Web Card asset is authoritative.
    if (-not (Test-Path -LiteralPath $BundledJs -PathType Leaf)) {
        if (Test-Path -LiteralPath $MasterJs -PathType Leaf) {
            Assert-WebCardVersion -Path $MasterJs
            return
        }
        throw ('安装包缺少 technical-specs-card.js：' + $BundledJs)
    }

    Assert-WebCardVersion -Path $BundledJs
    $samePath = [string]::Equals(
        [System.IO.Path]::GetFullPath($BundledJs),
        [System.IO.Path]::GetFullPath($MasterJs),
        [System.StringComparison]::OrdinalIgnoreCase
    )
    if (-not $samePath) {
        $copy = $true
        if (Test-Path -LiteralPath $MasterJs -PathType Leaf) {
            try {
                $srcHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $BundledJs).Hash
                $dstHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $MasterJs).Hash
                $copy = ($srcHash -ne $dstHash)
            } catch { $copy = $true }
        }
        if ($copy) {
            Save-BytesTransactional -Path $MasterJs -Bytes ([System.IO.File]::ReadAllBytes($BundledJs))
        }
    }
    Assert-WebCardVersion -Path $MasterJs
}

function Get-BytesSha256 {
    param([byte[]]$Bytes)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($sha.ComputeHash($Bytes))).Replace('-', '').ToLowerInvariant()
    }
    finally { $sha.Dispose() }
}

function Write-BytesDurableNew {
    param([string]$Path, [byte[]]$Bytes)
    $stream = New-Object System.IO.FileStream(
        $Path,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None,
        4096,
        [System.IO.FileOptions]::WriteThrough
    )
    try {
        $stream.Write($Bytes, 0, $Bytes.Length)
        $stream.Flush($true)
    }
    finally { $stream.Dispose() }
}

function Save-BytesTransactional {
    param([string]$Path, [byte[]]$Bytes)
    $dir = Split-Path -Parent $Path
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $tmp = Join-Path $dir ('.' + [System.IO.Path]::GetFileName($Path) + '.' + [guid]::NewGuid().ToString('N') + '.tmp')
    $rollback = Join-Path $dir ('.' + [System.IO.Path]::GetFileName($Path) + '.' + [guid]::NewGuid().ToString('N') + '.rollback')
    try {
        Write-BytesDurableNew -Path $tmp -Bytes $Bytes
        if (Test-Path -LiteralPath $Path -PathType Leaf) {
            [System.IO.File]::Replace($tmp, $Path, $rollback, $false)
            Remove-Item -LiteralPath $rollback -Force -ErrorAction SilentlyContinue
        }
        else {
            [System.IO.File]::Move($tmp, $Path)
        }
    }
    finally {
        Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    }
}

function Read-Utf8Text {
    param([string]$Path)

    # Windows PowerShell 5.1 treats a BOM-less UTF-8 file as the active ANSI
    # code page when Get-Content has no -Encoding. That can mojibake Chinese
    # and, worse, consume an ASCII quote as the second byte of a DBCS
    # character, turning otherwise valid JSON/XML into invalid syntax.
    # Our JSON and Emby Web files are UTF-8, so decode them explicitly.
    return [System.IO.File]::ReadAllText(
        $Path,
        [System.Text.UTF8Encoding]::new($false, $true)
    )
}

function Read-XmlDocument {
    param([string]$Path)

    # Let the XML parser read the raw file itself. It honors <?xml encoding=...?>
    # (tinyMediaManager writes UTF-8) and avoids PowerShell 5.1 ANSI decoding.
    try {
        $doc = New-Object System.Xml.XmlDocument
        $doc.PreserveWhitespace = $true
        $doc.Load($Path)
        return $doc
    }
    catch {
        $detail = $_.Exception.Message
        if ($_.Exception.InnerException -and $_.Exception.InnerException.Message) {
            $detail = $_.Exception.InnerException.Message
        }
        throw ('NFO XML 解析失败，已跳过：' + $detail)
    }
}

function Save-JsonAtomic {
    param($Object, [string]$Path, [int]$Depth = 16)
    $json = $Object | ConvertTo-Json -Depth $Depth -Compress
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($json)
    Save-BytesTransactional -Path $Path -Bytes $bytes
}

function Load-JsonMap {
    param([string]$Path)
    $map = @{}
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $map }
    try {
        $obj = Read-Utf8Text -Path $Path | ConvertFrom-Json
        if ($null -ne $obj) {
            foreach ($property in $obj.PSObject.Properties) {
                $map[$property.Name] = $property.Value
            }
        }
    }
    catch { return @{} }
    return $map
}

function Initialize-EmbySqliteReader {
    if ('EmbySqliteReader' -as [type]) {
        return
    }

    $source = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class EmbySqliteReader
{
    private const int SQLITE_OK = 0;
    private const int SQLITE_ROW = 100;
    private const int SQLITE_DONE = 101;
    private const int SQLITE_OPEN_READONLY = 0x00000001;
    private const int SQLITE_OPEN_URI = 0x00000040;

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl, CharSet = CharSet.Ansi)]
    private static extern int sqlite3_open_v2(
        string filename,
        out IntPtr db,
        int flags,
        IntPtr zvfs
    );

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_close(IntPtr db);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_prepare_v2(
        IntPtr db,
        byte[] sql,
        int nByte,
        out IntPtr stmt,
        IntPtr tail
    );

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_step(IntPtr stmt);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_finalize(IntPtr stmt);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_column_count(IntPtr stmt);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr sqlite3_column_name(IntPtr stmt, int iCol);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr sqlite3_column_text(IntPtr stmt, int iCol);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_column_bytes(IntPtr stmt, int iCol);

    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr sqlite3_errmsg(IntPtr db);

    private static string Utf8(IntPtr ptr)
    {
        if (ptr == IntPtr.Zero) return "";
        int len = 0;
        while (Marshal.ReadByte(ptr, len) != 0) len++;
        byte[] bytes = new byte[len];
        Marshal.Copy(ptr, bytes, 0, len);
        return Encoding.UTF8.GetString(bytes);
    }

    private static string ColumnText(IntPtr stmt, int index)
    {
        IntPtr ptr = sqlite3_column_text(stmt, index);
        if (ptr == IntPtr.Zero) return "";

        int len = sqlite3_column_bytes(stmt, index);
        if (len <= 0) return "";

        byte[] bytes = new byte[len];
        Marshal.Copy(ptr, bytes, 0, len);
        return Encoding.UTF8.GetString(bytes);
    }

    public static List<Dictionary<string, string>> Query(
        string dbPath,
        string sql
    )
    {
        IntPtr db = IntPtr.Zero;
        IntPtr stmt = IntPtr.Zero;

        int rc = sqlite3_open_v2(
            dbPath,
            out db,
            SQLITE_OPEN_READONLY | SQLITE_OPEN_URI,
            IntPtr.Zero
        );

        if (rc != SQLITE_OK)
        {
            string msg = db == IntPtr.Zero
                ? "sqlite3_open_v2 failed"
                : Utf8(sqlite3_errmsg(db));

            if (db != IntPtr.Zero) sqlite3_close(db);
            throw new Exception(msg);
        }

        try
        {
            byte[] sqlBytes = Encoding.UTF8.GetBytes(sql + "\0");

            rc = sqlite3_prepare_v2(
                db,
                sqlBytes,
                -1,
                out stmt,
                IntPtr.Zero
            );

            if (rc != SQLITE_OK)
                throw new Exception(Utf8(sqlite3_errmsg(db)));

            List<Dictionary<string, string>> rows =
                new List<Dictionary<string, string>>();

            int columnCount = sqlite3_column_count(stmt);

            while (true)
            {
                rc = sqlite3_step(stmt);

                if (rc == SQLITE_DONE)
                    break;

                if (rc != SQLITE_ROW)
                    throw new Exception(Utf8(sqlite3_errmsg(db)));

                Dictionary<string, string> row =
                    new Dictionary<string, string>(
                        StringComparer.OrdinalIgnoreCase
                    );

                for (int i = 0; i < columnCount; i++)
                {
                    string name = Utf8(sqlite3_column_name(stmt, i));
                    row[name] = ColumnText(stmt, i);
                }

                rows.Add(row);
            }

            return rows;
        }
        finally
        {
            if (stmt != IntPtr.Zero) sqlite3_finalize(stmt);
            if (db != IntPtr.Zero) sqlite3_close(db);
        }
    }
}
'@

    Add-Type -TypeDefinition $source -Language CSharp
}

function Convert-ToInt64 {
    param([string]$Value)

    $number = 0L

    if ([Int64]::TryParse(
        [string]$Value,
        [ref]$number
    )) {
        return $number
    }

    return 0L
}

function Get-MapEntries {
    param($Object)

    $entries = New-Object System.Collections.ArrayList

    if ($null -eq $Object) {
        return @()
    }

    if ($Object -is [System.Collections.IDictionary]) {
        foreach ($key in $Object.Keys) {
            [void]$entries.Add(
                [pscustomobject]@{
                    Name = [string]$key
                    Value = $Object[$key]
                }
            )
        }

        return @($entries)
    }

    foreach ($property in $Object.PSObject.Properties) {
        [void]$entries.Add(
            [pscustomobject]@{
                Name = [string]$property.Name
                Value = $property.Value
            }
        )
    }

    return @($entries)
}

function Get-PreviousLibraryMap {
    $map = @{}

    if (-not (Test-Path -LiteralPath $RootDiscoveryFile -PathType Leaf)) {
        return $map
    }

    try {
        $obj = Read-Utf8Text -Path $RootDiscoveryFile | ConvertFrom-Json

        foreach ($library in @($obj.libraries)) {
            $path = ([string]$library.Path).Trim()

            if ($path) {
                $map[$path] = $library
            }
        }
    }
    catch {}

    return $map
}

function Probe-RelevantNfoEvidence {
    param([string]$RootPath)

    $result = [ordered]@{
        Online = $false
        Movie = $false
        TV = $false
        CheckedNfo = 0
        Complete = $false
        HadErrors = $false
    }

    if (-not $RootPath) {
        return [pscustomobject]$result
    }

    if (-not (Test-Path -LiteralPath $RootPath -PathType Container)) {
        return [pscustomobject]$result
    }

    $result.Online = $true
    $scanErrors = @()

    try {
        $nfoFiles = @(
            Get-ChildItem `
                -LiteralPath $RootPath `
                -Recurse `
                -File `
                -Filter '*.nfo' `
                -ErrorAction SilentlyContinue `
                -ErrorVariable +scanErrors
        )
    }
    catch {
        $scanErrors += $_
        $nfoFiles = @()
    }

    foreach ($file in $nfoFiles) {
        $result.CheckedNfo++

        try {
            $xml = Read-XmlDocument -Path $file.FullName
            $root = $xml.DocumentElement

            if ($null -eq $root) {
                continue
            }

            $rootName = ([string]$root.Name).ToLowerInvariant()

            if ($rootName -eq 'movie') {
                $hasImdb = $false

                foreach ($node in @($root.SelectNodes('./uniqueid'))) {
                    if (
                        ([string]$node.GetAttribute('type')).Trim() -ieq 'imdb' -and
                        ([string]$node.InnerText).Trim() -match '^tt\d{5,12}$'
                    ) {
                        $hasImdb = $true
                        break
                    }
                }

                if (-not $hasImdb) {
                    $legacy = ([string]$root.imdbid).Trim()
                    $hasImdb = ($legacy -match '^tt\d{5,12}$')
                }

                if (-not $hasImdb) {
                    foreach ($node in @($root.SelectNodes('./technicalspecs'))) {
                        if (
                            ([string]$node.GetAttribute('source')).Trim() -ieq 'IMDb' -and
                            ([string]$node.GetAttribute('imdbid')).Trim() -match '^tt\d{5,12}$'
                        ) {
                            $hasImdb = $true
                            break
                        }
                    }
                }

                if ($hasImdb) {
                    $result.Movie = $true
                }
            }
            elseif (
                $rootName -eq 'tvshow' -or
                $rootName -eq 'episodedetails' -or
                $rootName -eq 'season'
            ) {
                $result.TV = $true
            }

            if ($result.Movie -and $result.TV) {
                break
            }
        }
        catch {
            # One malformed NFO must not prevent classification of the root.
        }
    }

    if ($scanErrors.Count -gt 0) {
        $result.HadErrors = $true
        $result.Complete = $false
    }
    else {
        $result.Complete = $true
    }

    return [pscustomobject]$result
}

function Get-EmbyMediaLibraries {
    if (-not (Test-Path -LiteralPath $LibraryDb -PathType Leaf)) {
        throw "找不到 Emby library.db：$LibraryDb"
    }

    Initialize-EmbySqliteReader

    # Current discovery rules:
    #
    # 1. Physical roots are taken from the real MediaItems tree, never by
    #    collapsing common filesystem parents.
    # 2. A root is eligible when it is a physical folder directly under the
    #    global root, OR under a virtual wrapper whose own Path is empty.
    #    This covers one virtual library with multiple physical paths.
    # 3. TV evidence comes from Series/Episode descendants (4.9.5 codes 6/8).
    # 4. Movie evidence does NOT use IsMovie because this user's real movies
    #    have IsMovie blank. Instead, type=5 items with an IMDb provider id are
    #    strong evidence; ambiguous video roots get an NFO fallback probe.
    # 5. Music and generic home-video roots are ignored unless the NFO fallback
    #    proves they actually contain movie/tv NFOs relevant to this indexer.
    $sql = @"
WITH RECURSIVE
candidates(Id, ParentId, Name, Path) AS (
    SELECT
        child.Id,
        child.ParentId,
        child.Name,
        child.Path
    FROM MediaItems AS child
    LEFT JOIN MediaItems AS parent
        ON parent.Id = child.ParentId
    WHERE child.type = 3
      AND child.Path IS NOT NULL
      AND trim(child.Path) <> ''
      AND (
            child.ParentId = 2
         OR parent.Path IS NULL
         OR trim(parent.Path) = ''
      )
),
tree(RootId, Id) AS (
    SELECT Id, Id
    FROM candidates

    UNION ALL

    SELECT
        tree.RootId,
        child.Id
    FROM MediaItems AS child
    INNER JOIN tree
        ON child.ParentId = tree.Id
    WHERE child.type IN (3, 5, 6, 7, 8)
)
SELECT
    candidates.Id AS RootId,
    candidates.ParentId AS RootParentId,
    candidates.Name AS RootName,
    candidates.Path AS RootPath,
    SUM(CASE WHEN item.type = 6 THEN 1 ELSE 0 END) AS SeriesCount,
    SUM(CASE WHEN item.type = 8 THEN 1 ELSE 0 END) AS EpisodeCount,
    SUM(CASE WHEN item.type = 5 THEN 1 ELSE 0 END) AS VideoItemCount,
    SUM(
        CASE
            WHEN item.type = 5
             AND lower(COALESCE(item.ProviderIds, '')) LIKE '%imdb=tt%'
             AND (
                    item.ExtraType IS NULL
                 OR trim(CAST(item.ExtraType AS TEXT)) = ''
                 OR CAST(item.ExtraType AS TEXT) = '0'
             )
            THEN 1
            ELSE 0
        END
    ) AS ImdbVideoCount
FROM candidates
INNER JOIN tree
    ON tree.RootId = candidates.Id
INNER JOIN MediaItems AS item
    ON item.Id = tree.Id
GROUP BY
    candidates.Id,
    candidates.ParentId,
    candidates.Name,
    candidates.Path
ORDER BY
    candidates.Id
"@

    $rows = [EmbySqliteReader]::Query(
        $LibraryDb,
        $sql
    )

    $previous = Get-PreviousLibraryMap
    $result = New-Object System.Collections.ArrayList

    foreach ($row in $rows) {
        $path = ([string]$row['RootPath']).Trim()
        $name = ([string]$row['RootName']).Trim()

        if (-not $path) {
            continue
        }

        $seriesCount = Convert-ToInt64 ([string]$row['SeriesCount'])
        $episodeCount = Convert-ToInt64 ([string]$row['EpisodeCount'])
        $videoItemCount = Convert-ToInt64 ([string]$row['VideoItemCount'])
        $imdbVideoCount = Convert-ToInt64 ([string]$row['ImdbVideoCount'])
        $online = Test-Path -LiteralPath $path -PathType Container

        $hasTV = ($seriesCount -gt 0 -or $episodeCount -gt 0)
        $hasMovie = ($imdbVideoCount -gt 0)
        $probe = $null
        $probeNeeded = (
            -not $hasMovie -and
            -not $hasTV -and
            $videoItemCount -gt 0 -and
            $online
        )

        if ($probeNeeded) {
            $probe = Probe-RelevantNfoEvidence -RootPath $path

            if ($probe.Movie) {
                $hasMovie = $true
            }

            if ($probe.TV) {
                $hasTV = $true
            }
        }

        # If a root is temporarily offline, retain a previously proven
        # classification instead of silently dropping the entire cache.
        if (-not $hasMovie -and -not $hasTV -and -not $online) {
            if ($previous.ContainsKey($path)) {
                $oldKind = ([string]$previous[$path].Kind).Trim()

                if ($oldKind -match 'Movie') {
                    $hasMovie = $true
                }

                if ($oldKind -match 'TV') {
                    $hasTV = $true
                }
            }
        }

        $kind = 'Ignored'
        $included = $false

        if ($hasMovie -and $hasTV) {
            $kind = 'Mixed Movie/TV'
            $included = $true
        }
        elseif ($hasTV) {
            $kind = 'TV'
            $included = $true
        }
        elseif ($hasMovie) {
            $kind = 'Movies'
            $included = $true
        }

        $evidence = New-Object System.Collections.ArrayList

        if ($imdbVideoCount -gt 0) {
            [void]$evidence.Add(('IMDb-video=' + $imdbVideoCount))
        }

        if ($seriesCount -gt 0) {
            [void]$evidence.Add(('Series=' + $seriesCount))
        }

        if ($episodeCount -gt 0) {
            [void]$evidence.Add(('Episode=' + $episodeCount))
        }

        if ($null -ne $probe) {
            if ($probe.Movie) {
                [void]$evidence.Add('NFO=movie')
            }

            if ($probe.TV) {
                [void]$evidence.Add('NFO=tv')
            }

            if (-not $probe.Movie -and -not $probe.TV) {
                [void]$evidence.Add(('NFO-probe=' + $probe.CheckedNfo))
            }
        }

        if ($evidence.Count -eq 0) {
            [void]$evidence.Add(('video-items=' + $videoItemCount))
        }

        [void]$result.Add(
            [pscustomobject]@{
                Id = Convert-ToInt64 ([string]$row['RootId'])
                ParentId = Convert-ToInt64 ([string]$row['RootParentId'])
                Name = $name
                Path = $path
                Kind = $kind
                Included = $included
                Online = [bool]$online
                ImdbVideoCount = $imdbVideoCount
                SeriesCount = $seriesCount
                EpisodeCount = $episodeCount
                VideoItemCount = $videoItemCount
                Evidence = (@($evidence) -join ', ')
            }
        )
    }

    $snapshot = [ordered]@{
        generatedAt = (Get-Date).ToUniversalTime().ToString('o')
        libraries = @($result)
    }

    Save-JsonAtomic -Object $snapshot -Path $RootDiscoveryFile -Depth 10

    return @($result)
}

function Get-EmbyLibraryRoots {
    $libraries = @(Get-EmbyMediaLibraries)
    $paths = New-Object System.Collections.ArrayList

    foreach ($library in $libraries) {
        if (-not $library.Included) {
            continue
        }

        $path = ([string]$library.Path).Trim()

        if ($path -and -not $paths.Contains($path)) {
            [void]$paths.Add($path)
        }
    }

    return @($paths)
}

function Get-SelectedMediaLibraries {
    $discovered = @(Get-EmbyMediaLibraries)

    if (-not $RootConfigPath -or -not (Test-Path -LiteralPath $RootConfigPath -PathType Leaf)) {
        return @($discovered | Where-Object { $_.Included })
    }

    try {
        $settings = Read-Utf8Text -Path $RootConfigPath | ConvertFrom-Json
    }
    catch {
        throw ('读取媒体目录设置失败：' + $_.Exception.Message)
    }

    if (-not [bool]$settings.roots_configured) {
        return @($discovered | Where-Object { $_.Included })
    }

    $selected = New-Object System.Collections.ArrayList
    foreach ($configured in @($settings.library_roots)) {
        if (-not [bool]$configured.enabled) {
            continue
        }

        $path = ([string]$configured.path).Trim()
        if (-not $path) {
            continue
        }

        $match = @(
            $discovered | Where-Object {
                [string]::Equals(
                    ([string]$_.Path).TrimEnd([char]92, [char]47),
                    $path.TrimEnd([char]92, [char]47),
                    [System.StringComparison]::OrdinalIgnoreCase
                )
            }
        ) | Select-Object -First 1

        if ($null -ne $match) {
            $configuredName = ([string]$configured.name).Trim()
            if (-not $configuredName) {
                $configuredName = $match.Name
            }
            [void]$selected.Add(
                [pscustomobject]@{
                    Id = $match.Id
                    ParentId = $match.ParentId
                    Name = $configuredName
                    Path = $path
                    Kind = $match.Kind
                    Included = $true
                    Online = Test-Path -LiteralPath $path -PathType Container
                    ImdbVideoCount = $match.ImdbVideoCount
                    SeriesCount = $match.SeriesCount
                    EpisodeCount = $match.EpisodeCount
                    VideoItemCount = $match.VideoItemCount
                    Evidence = $match.Evidence + ', 用户已选择'
                    Source = [string]$configured.source
                }
            )
            continue
        }

        $kind = '用户选择'
        switch (([string]$configured.kind).ToLowerInvariant()) {
            'movies' { $kind = 'Movies' }
            'tv' { $kind = 'TV' }
            'mixed' { $kind = 'Mixed Movie/TV' }
        }
        $name = ([string]$configured.name).Trim()
        if (-not $name) {
            $name = Split-Path -Leaf $path
        }
        [void]$selected.Add(
            [pscustomobject]@{
                Id = 0
                ParentId = 0
                Name = $name
                Path = $path
                Kind = $kind
                Included = $true
                Online = Test-Path -LiteralPath $path -PathType Container
                ImdbVideoCount = 0
                SeriesCount = 0
                EpisodeCount = 0
                VideoItemCount = 0
                Evidence = '用户手动选择'
                Source = [string]$configured.source
            }
        )
    }

    if ($selected.Count -eq 0) {
        throw '媒体目录设置中没有启用的目录。'
    }

    return @($selected)
}

function Test-PathWithinRoot {
    param(
        [string]$Path,
        [string]$Root
    )

    if (-not $Path -or -not $Root) {
        return $false
    }

    try {
        $fullPath = [System.IO.Path]::GetFullPath($Path).TrimEnd([char]92, [char]47)
        $fullRoot = [System.IO.Path]::GetFullPath($Root).TrimEnd([char]92, [char]47)

        if ($fullPath.Equals(
            $fullRoot,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
            return $true
        }

        $prefix = $fullRoot + [char]92

        return $fullPath.StartsWith(
            $prefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )
    }
    catch {
        return $false
    }
}

function Test-PathWithinAnyRoot {
    param(
        [string]$Path,
        [object[]]$Roots
    )

    foreach ($root in @($Roots)) {
        if (Test-PathWithinRoot -Path $Path -Root ([string]$root)) {
            return $true
        }
    }

    return $false
}

function Load-RootState {
    if (-not (Test-Path -LiteralPath $RootStateFile -PathType Leaf)) {
        return @()
    }

    try {
        $obj = Read-Utf8Text -Path $RootStateFile | ConvertFrom-Json
        return @($obj.roots | ForEach-Object { [string]$_ })
    }
    catch {
        return @()
    }
}

function Save-RootState {
    param([object[]]$Roots)

    $obj = [ordered]@{
        savedAt = (Get-Date).ToUniversalTime().ToString('o')
        roots = @($Roots)
    }

    Save-JsonAtomic -Object $obj -Path $RootStateFile -Depth 6
}

function Get-WebPatchContext {
    if (-not $BackupRootPath) { throw 'WEB_PATCH_BACKUP_ROOT_REQUIRED：缺少 Portable backup 路径。' }
    $indexFull = [System.IO.Path]::GetFullPath($IndexHtml)
    $embyFull = [System.IO.Path]::GetFullPath($EmbyRoot).TrimEnd('\') + '\'
    $backupFull = [System.IO.Path]::GetFullPath($BackupRootPath).TrimEnd('\') + '\'
    if ($backupFull.StartsWith($embyFull, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'WEB_PATCH_BACKUP_INSIDE_EMBY：长期备份不能放在 Emby 管理的目录树中。'
    }
    $idBytes = [System.Text.UTF8Encoding]::new($false).GetBytes($indexFull.ToLowerInvariant())
    $instanceId = (Get-BytesSha256 -Bytes $idBytes).Substring(0, 20)
    $instanceDir = Join-Path $backupFull ('emby-' + $instanceId)
    New-Item -ItemType Directory -Force -Path $instanceDir | Out-Null
    return [pscustomobject]@{
        InstanceId = $instanceId
        InstanceDir = $instanceDir
        StatePath = Join-Path $instanceDir 'web-patch-state.json'
        JournalPath = Join-Path $instanceDir 'web-patch-transaction.json'
        LockPath = Join-Path $instanceDir 'web-patch.lock'
        MutexName = 'Global\IMDbTechManager.EmbyWebPatch.' + $instanceId
    }
}

function Get-FileSnapshot {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw ('FILE_MISSING：' + $Path) }
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $hasBom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xef -and $bytes[1] -eq 0xbb -and $bytes[2] -eq 0xbf
    $offset = if ($hasBom) { 3 } else { 0 }
    $utf8 = [System.Text.UTF8Encoding]::new($false, $true)
    $text = $utf8.GetString($bytes, $offset, $bytes.Length - $offset)
    $newline = if ($text.Contains("`r`n")) { "`r`n" } else { "`n" }
    return [pscustomobject]@{
        Path = $Path
        Bytes = $bytes
        Hash = Get-BytesSha256 -Bytes $bytes
        Text = $text
        HasBom = $hasBom
        Newline = $newline
    }
}

function Convert-IndexTextToBytes {
    param([string]$Text, [bool]$HasBom)
    $payload = [System.Text.UTF8Encoding]::new($false, $true).GetBytes($Text)
    if (-not $HasBom) { return ,$payload }
    $bytes = New-Object byte[] ($payload.Length + 3)
    $bytes[0] = 0xef; $bytes[1] = 0xbb; $bytes[2] = 0xbf
    [System.Array]::Copy($payload, 0, $bytes, 3, $payload.Length)
    return ,$bytes
}

function Get-OwnedScriptPattern {
    return '<script\b[^>]*\bsrc=["'']technical-specs-card\.js\?v=[^"'']+["''][^>]*>\s*</script>'
}

function Assert-EmbyIndexSnapshot {
    param($Snapshot, [switch]$AllowOwnedPatch)
    if ($Snapshot.Bytes.Length -lt 256) { throw 'INVALID_EMBY_INDEX：文件过小。' }
    $text = [string]$Snapshot.Text
    foreach ($required in @('<html', '<head', '<body', '</body>', '</html>')) {
        if ($text.IndexOf($required, [System.StringComparison]::OrdinalIgnoreCase) -lt 0) {
            throw ('INVALID_EMBY_INDEX：缺少 ' + $required)
        }
    }
    if ($text.IndexOf([char]0) -ge 0) { throw 'INVALID_EMBY_INDEX：包含 NUL。' }
    $beginCount = [regex]::Matches($text, [regex]::Escape($PatchBegin)).Count
    $endCount = [regex]::Matches($text, [regex]::Escape($PatchEnd)).Count
    if ($beginCount -ne $endCount -or $beginCount -gt 1) { throw 'INVALID_WEB_PATCH_MARKERS：受管标记不完整或重复。' }
    $scriptCount = [regex]::Matches($text, (Get-OwnedScriptPattern), [System.Text.RegularExpressions.RegexOptions]::IgnoreCase).Count
    if (-not $AllowOwnedPatch -and ($beginCount -gt 0 -or $scriptCount -gt 0)) {
        throw 'BASELINE_CONTAINS_WEB_PATCH：原版备份不得包含 Technical Specs 注入。'
    }
    if ($scriptCount -gt 8) { throw 'INVALID_WEB_PATCH_COUNT：Technical Specs script 数量异常。' }
}

function Get-CleanIndexText {
    param($Snapshot)
    Assert-EmbyIndexSnapshot -Snapshot $Snapshot -AllowOwnedPatch
    $text = [string]$Snapshot.Text
    $hadPatch = $false
    $beginCount = [regex]::Matches($text, [regex]::Escape($PatchBegin)).Count
    $endCount = [regex]::Matches($text, [regex]::Escape($PatchEnd)).Count
    $ownedPattern = Get-OwnedScriptPattern
    $scriptCount = [regex]::Matches(
        $text,
        $ownedPattern,
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    ).Count
    $strictOwnedPatch = $false
    if ($beginCount -eq 1 -and $endCount -eq 1) {
        $beginAt = $text.IndexOf($PatchBegin, [System.StringComparison]::Ordinal)
        $endAt = $text.IndexOf($PatchEnd, [System.StringComparison]::Ordinal)
        if ($beginAt -ge 0 -and $endAt -gt ($beginAt + $PatchBegin.Length)) {
            $insideAt = $beginAt + $PatchBegin.Length
            $inside = $text.Substring($insideAt, $endAt - $insideAt)
            $insideWithoutScript = [regex]::Replace(
                $inside,
                $ownedPattern,
                '',
                [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
            )
            $insideScriptCount = [regex]::Matches(
                $inside,
                $ownedPattern,
                [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
            ).Count
            $strictOwnedPatch = $insideScriptCount -eq 1 -and $scriptCount -eq 1 -and [string]::IsNullOrWhiteSpace($insideWithoutScript)
        }
    }
    if (($beginCount -gt 0 -or $endCount -gt 0 -or $scriptCount -gt 0) -and -not $strictOwnedPatch) {
        throw 'UNSAFE_WEB_PATCH_OWNERSHIP：检测到不完整、重复或夹带其它内容的网页补丁，拒绝自动删除或覆盖。'
    }
    if ($strictOwnedPatch) {
        $managedPattern = '(?s)[ \t]*' + [regex]::Escape($PatchBegin) + '.*?' + [regex]::Escape($PatchEnd) + '[ \t]*(?:\r?\n)?'
        $text = [regex]::Replace($text, $managedPattern, '', [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
        $hadPatch = $true
    }
    $cleanBytes = Convert-IndexTextToBytes -Text $text -HasBom $Snapshot.HasBom
    $cleanSnapshot = [pscustomobject]@{
        Path = $Snapshot.Path
        Bytes = $cleanBytes
        Hash = Get-BytesSha256 -Bytes $cleanBytes
        Text = $text
        HasBom = $Snapshot.HasBom
        Newline = $Snapshot.Newline
    }
    Assert-EmbyIndexSnapshot -Snapshot $cleanSnapshot
    return [pscustomobject]@{
        Snapshot = $cleanSnapshot
        HadPatch = $hadPatch
        StrictOwnedPatch = $strictOwnedPatch
    }
}

function Read-WebPatchState {
    param($Context)
    if (-not (Test-Path -LiteralPath $Context.StatePath -PathType Leaf)) { return $null }
    try { return Read-Utf8Text -Path $Context.StatePath | ConvertFrom-Json }
    catch { throw ('WEB_PATCH_STATE_INVALID：' + $_.Exception.Message) }
}

function Save-ImmutableBaseline {
    param($Context, $Snapshot)
    Assert-EmbyIndexSnapshot -Snapshot $Snapshot
    $path = Join-Path $Context.InstanceDir ('index.original.' + $Snapshot.Hash + '.html')
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        Write-BytesDurableNew -Path $path -Bytes $Snapshot.Bytes
    }
    $verify = Get-FileSnapshot -Path $path
    Assert-EmbyIndexSnapshot -Snapshot $verify
    if ($verify.Hash -ne $Snapshot.Hash) { throw 'BASELINE_HASH_MISMATCH：不可变备份校验失败。' }
    return $verify
}

function Resolve-TrustedBaseline {
    param($Context, $CurrentSnapshot, $CleanResult)
    if (-not $CleanResult.HadPatch) {
        return Save-ImmutableBaseline -Context $Context -Snapshot $CurrentSnapshot
    }
    $candidates = New-Object System.Collections.ArrayList
    $state = Read-WebPatchState -Context $Context
    if ($state -and $state.baselinePath) {
        $stateBaseline = [System.IO.Path]::GetFullPath([string]$state.baselinePath)
        $instancePrefix = [System.IO.Path]::GetFullPath($Context.InstanceDir).TrimEnd('\') + '\'
        if (-not $stateBaseline.StartsWith($instancePrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw 'WEB_PATCH_STATE_BASELINE_OUTSIDE_BACKUP：状态文件中的原版备份路径越界。'
        }
        [void]$candidates.Add($stateBaseline)
    }
    if (Test-Path -LiteralPath $Backup -PathType Leaf) { [void]$candidates.Add($Backup) }
    foreach ($path in @($candidates | Select-Object -Unique)) {
        try {
            $candidate = Get-FileSnapshot -Path $path
            Assert-EmbyIndexSnapshot -Snapshot $candidate
            if ($candidate.Text -eq $CleanResult.Snapshot.Text) {
                return Save-ImmutableBaseline -Context $Context -Snapshot $candidate
            }
        }
        catch {}
    }
    # A newly extracted Portable version cannot assume the previous folder is
    # still reachable.  For one exact Manager-owned marker block, the current
    # index with only that block removed is the authoritative clean baseline.
    # All other Emby and third-party bytes remain untouched.
    if ($CleanResult.StrictOwnedPatch) {
        return Save-ImmutableBaseline -Context $Context -Snapshot $CleanResult.Snapshot
    }
    throw 'NO_TRUSTED_BASELINE：当前 index 已注入，但没有与其干净内容匹配的可靠原版备份。请先用同版本 Emby 官方安装包修复 Web UI。'
}

function Assert-TransactionPath {
    param([string]$Path)
    $parent = [System.IO.Path]::GetFullPath((Split-Path -Parent $Path)).TrimEnd('\')
    $dashboard = [System.IO.Path]::GetFullPath($DashboardUi).TrimEnd('\')
    if (-not $parent.Equals($dashboard, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'WEB_PATCH_JOURNAL_PATH_INVALID：事务文件不在 dashboard-ui。'
    }
}

function Recover-WebPatchTransaction {
    param($Context)
    if (-not (Test-Path -LiteralPath $Context.JournalPath -PathType Leaf)) { return }
    $journal = Read-Utf8Text -Path $Context.JournalPath | ConvertFrom-Json
    $journalIndex = [System.IO.Path]::GetFullPath([string]$journal.indexPath)
    $expectedIndex = [System.IO.Path]::GetFullPath($IndexHtml)
    if (-not $journalIndex.Equals($expectedIndex, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'WEB_PATCH_JOURNAL_INDEX_INVALID：事务日志不属于当前 Emby index.html。'
    }
    if ([string]$journal.sourceHash -notmatch '^[a-f0-9]{64}$' -or [string]$journal.candidateHash -notmatch '^[a-f0-9]{64}$') {
        throw 'WEB_PATCH_JOURNAL_HASH_INVALID：事务日志 Hash 无效。'
    }
    Assert-TransactionPath -Path ([string]$journal.tempPath)
    Assert-TransactionPath -Path ([string]$journal.rollbackPath)
    $current = if (Test-Path -LiteralPath $IndexHtml -PathType Leaf) { Get-FileSnapshot -Path $IndexHtml } else { $null }
    $rollbackValid = $false
    if (Test-Path -LiteralPath ([string]$journal.rollbackPath) -PathType Leaf) {
        $rollback = Get-FileSnapshot -Path ([string]$journal.rollbackPath)
        $rollbackValid = ($rollback.Hash -eq [string]$journal.sourceHash)
    }
    if ($current -and $current.Hash -eq [string]$journal.sourceHash) {
        # Replacement never happened or a prior recovery already restored it.
    }
    elseif ($current -and $current.Hash -eq [string]$journal.candidateHash) {
        if ([string]$journal.phase -eq 'COMMITTED') {
            Assert-EmbyIndexSnapshot -Snapshot $current -AllowOwnedPatch
            $hasOwnedPatch = $current.Text.Contains($PatchBegin) -or [regex]::IsMatch($current.Text, (Get-OwnedScriptPattern), [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
            if ([bool]$journal.expectPatch -ne [bool]$hasOwnedPatch) {
                throw 'WEB_PATCH_RECOVERY_FAILED：已提交候选与日志声明的 Patch 状态不一致。'
            }
        }
        elseif ($rollbackValid) {
            $recoveredCandidate = Join-Path $DashboardUi ('.index.html.' + [guid]::NewGuid().ToString('N') + '.recovered-candidate')
            [System.IO.File]::Replace([string]$journal.rollbackPath, $IndexHtml, $recoveredCandidate, $false)
            $restored = Get-FileSnapshot -Path $IndexHtml
            if ($restored.Hash -ne [string]$journal.sourceHash) { throw 'WEB_PATCH_RECOVERY_FAILED：回滚后 Hash 不一致。' }
            Remove-Item -LiteralPath $recoveredCandidate -Force -ErrorAction SilentlyContinue
        }
        else {
            throw 'WEB_PATCH_RECOVERY_REQUIRED：候选已替换但可靠回滚文件缺失。'
        }
    }
    elseif (-not $current -and $rollbackValid) {
        [System.IO.File]::Move([string]$journal.rollbackPath, $IndexHtml)
        $restored = Get-FileSnapshot -Path $IndexHtml
        if ($restored.Hash -ne [string]$journal.sourceHash) { throw 'WEB_PATCH_RECOVERY_FAILED：缺失入口恢复后 Hash 不一致。' }
    }
    else {
        throw 'WEB_PATCH_RECOVERY_REQUIRED：检测到未完成事务和第三方文件变化，已停止自动处理。'
    }
    Remove-Item -LiteralPath ([string]$journal.tempPath) -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath ([string]$journal.rollbackPath) -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $Context.JournalPath -Force -ErrorAction SilentlyContinue
}

function Invoke-WithWebPatchLock {
    param([scriptblock]$Body)
    $context = Get-WebPatchContext
    $mutex = [System.Threading.Mutex]::new($false, $context.MutexName)
    $acquired = $false
    $abandoned = $false
    $lockStream = $null
    try {
        try { $acquired = $mutex.WaitOne([TimeSpan]::FromSeconds(30)) }
        catch [System.Threading.AbandonedMutexException] { $acquired = $true; $abandoned = $true }
        if (-not $acquired) { throw 'WEB_PATCH_BUSY：另一个 Web Patch 事务仍在运行。' }
        $lockStream = [System.IO.FileStream]::new(
            $context.LockPath,
            [System.IO.FileMode]::OpenOrCreate,
            [System.IO.FileAccess]::ReadWrite,
            [System.IO.FileShare]::None
        )
        if ($abandoned -or (Test-Path -LiteralPath $context.JournalPath -PathType Leaf)) {
            Recover-WebPatchTransaction -Context $context
        }
        return & $Body $context
    }
    finally {
        if ($lockStream) { $lockStream.Dispose() }
        if ($acquired) { try { $mutex.ReleaseMutex() } catch {} }
        $mutex.Dispose()
    }
}

function Sync-LiveWebCardTransactional {
    $source = Get-FileSnapshot -Path $MasterJs
    if (Test-Path -LiteralPath $LiveJs -PathType Leaf) {
        $current = Get-FileSnapshot -Path $LiveJs
        if ($current.Hash -eq $source.Hash) { Assert-WebCardVersion -Path $LiveJs; return }
    }
    Save-BytesTransactional -Path $LiveJs -Bytes $source.Bytes
    $verify = Get-FileSnapshot -Path $LiveJs
    if ($verify.Hash -ne $source.Hash) { throw 'WEB_CARD_ASSET_HASH_MISMATCH：前端脚本替换校验失败。' }
    Assert-WebCardVersion -Path $LiveJs
}

function Save-WebPatchState {
    param($Context, $Baseline, [string]$CleanHash, [string]$PatchedHash)
    Save-JsonAtomic -Object ([ordered]@{
        schemaVersion = 1
        managerVersion = $ManagerVersion
        instanceId = $Context.InstanceId
        indexPath = [System.IO.Path]::GetFullPath($IndexHtml)
        baselinePath = $Baseline.Path
        baselineSha256 = $Baseline.Hash
        cleanSha256 = $CleanHash
        patchedSha256 = $PatchedHash
        webCardVersion = $ExpectedWebCardVersion
        updatedAt = (Get-Date).ToUniversalTime().ToString('o')
    }) -Path $Context.StatePath -Depth 8
}

function Replace-EmbyIndexTransactional {
    param($Context, $Source, [byte[]]$CandidateBytes, $Baseline, [switch]$ExpectPatch)
    $candidateHash = Get-BytesSha256 -Bytes $CandidateBytes
    if ($candidateHash -eq $Source.Hash) {
        Save-WebPatchState -Context $Context -Baseline $Baseline -CleanHash $Baseline.Hash -PatchedHash $candidateHash
        return $false
    }
    $tmp = Join-Path $DashboardUi ('.index.html.techspec.' + [guid]::NewGuid().ToString('N') + '.tmp')
    $rollback = Join-Path $DashboardUi ('.index.html.techspec.' + [guid]::NewGuid().ToString('N') + '.rollback')
    $journal = [ordered]@{
        schemaVersion = 1; phase = 'PREPARING'; indexPath = $IndexHtml
        tempPath = $tmp; rollbackPath = $rollback
        sourceHash = $Source.Hash; candidateHash = $candidateHash
        baselinePath = $Baseline.Path; baselineHash = $Baseline.Hash
        expectPatch = [bool]$ExpectPatch
        startedAt = (Get-Date).ToUniversalTime().ToString('o')
    }
    Save-JsonAtomic -Object $journal -Path $Context.JournalPath -Depth 8
    $committed = $false
    try {
        Write-BytesDurableNew -Path $tmp -Bytes $CandidateBytes
        $tmpSnapshot = Get-FileSnapshot -Path $tmp
        if ($tmpSnapshot.Hash -ne $candidateHash) { throw 'WEB_PATCH_CANDIDATE_HASH_MISMATCH。' }
        Assert-EmbyIndexSnapshot -Snapshot $tmpSnapshot -AllowOwnedPatch
        $journal.phase = 'CANDIDATE_VERIFIED'
        Save-JsonAtomic -Object $journal -Path $Context.JournalPath -Depth 8

        $cas = Get-FileSnapshot -Path $IndexHtml
        if ($cas.Hash -ne $Source.Hash) { throw 'CONCURRENT_MODIFICATION：Emby index.html 在读取后被其它程序修改，事务已中止。' }
        [System.IO.File]::Replace($tmp, $IndexHtml, $rollback, $false)
        $journal.phase = 'REPLACED'
        Save-JsonAtomic -Object $journal -Path $Context.JournalPath -Depth 8

        $installed = Get-FileSnapshot -Path $IndexHtml
        Assert-EmbyIndexSnapshot -Snapshot $installed -AllowOwnedPatch
        if ($installed.Hash -ne $candidateHash) { throw 'WEB_PATCH_POST_VERIFY_FAILED：正式文件 Hash 不一致。' }
        $hasMarker = $installed.Text.Contains($PatchBegin) -and $installed.Text.Contains($PatchEnd)
        $hasTargetScript = $installed.Text.Contains((Get-WebCardScriptTag))
        $hasOwnedScript = [regex]::IsMatch($installed.Text, (Get-OwnedScriptPattern), [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
        if ($ExpectPatch -and (-not $hasMarker -or -not $hasTargetScript)) {
            throw 'WEB_PATCH_POST_VERIFY_FAILED：受管块或目标 script 缺失。'
        }
        if (-not $ExpectPatch -and ($hasMarker -or $hasOwnedScript)) {
            throw 'WEB_PATCH_POST_VERIFY_FAILED：恢复结果仍含受管 Web Patch。'
        }
        Save-WebPatchState -Context $Context -Baseline $Baseline -CleanHash $Baseline.Hash -PatchedHash $candidateHash
        $journal.phase = 'COMMITTED'
        Save-JsonAtomic -Object $journal -Path $Context.JournalPath -Depth 8
        $committed = $true
        return $true
    }
    catch {
        if (-not $committed -and (Test-Path -LiteralPath $rollback -PathType Leaf) -and (Test-Path -LiteralPath $IndexHtml -PathType Leaf)) {
            try {
                $now = Get-FileSnapshot -Path $IndexHtml
                $old = Get-FileSnapshot -Path $rollback
                if ($now.Hash -eq $candidateHash -and $old.Hash -eq $Source.Hash) {
                    $failedCandidate = Join-Path $DashboardUi ('.index.html.techspec.' + [guid]::NewGuid().ToString('N') + '.failed')
                    [System.IO.File]::Replace($rollback, $IndexHtml, $failedCandidate, $false)
                    $restored = Get-FileSnapshot -Path $IndexHtml
                    if ($restored.Hash -ne $Source.Hash) { throw '条件回滚后的 Hash 不一致。' }
                    Remove-Item -LiteralPath $failedCandidate -Force -ErrorAction SilentlyContinue
                }
            }
            catch { throw ('WEB_PATCH_ROLLBACK_FAILED：' + $_.Exception.Message) }
        }
        throw
    }
    finally {
        if ($committed) {
            Remove-Item -LiteralPath $rollback -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $Context.JournalPath -Force -ErrorAction SilentlyContinue
        }
        Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    }
}

function Install-WebPatch {
    if (-not (Test-Path -LiteralPath $DashboardUi -PathType Container)) { throw "找不到 Emby dashboard-ui：$DashboardUi" }
    if (-not (Test-Path -LiteralPath $IndexHtml -PathType Leaf)) { throw 'EMBY_INDEX_MISSING：请先用当前版本 Emby 官方安装包修复 Web UI；本工具不会用旧版备份盲目覆盖。' }
    if (-not (Test-Path -LiteralPath $MasterJs -PathType Leaf)) { throw "找不到技术规格前端脚本：$MasterJs" }
    Invoke-WithWebPatchLock -Body {
        param($context)
        $current = Get-FileSnapshot -Path $IndexHtml
        $clean = Get-CleanIndexText -Snapshot $current
        $baseline = Resolve-TrustedBaseline -Context $context -CurrentSnapshot $current -CleanResult $clean
        $liveBefore = if (Test-Path -LiteralPath $LiveJs -PathType Leaf) { Get-FileSnapshot -Path $LiveJs } else { $null }
        $masterSnapshot = Get-FileSnapshot -Path $MasterJs
        Sync-LiveWebCardTransactional
        $newline = $clean.Snapshot.Newline
        $block = '  ' + $PatchBegin + $newline + '  ' + (Get-WebCardScriptTag) + $newline + '  ' + $PatchEnd + $newline
        $bodyPos = $clean.Snapshot.Text.LastIndexOf('</body>', [System.StringComparison]::OrdinalIgnoreCase)
        if ($bodyPos -lt 0) { throw 'INVALID_EMBY_INDEX：无法定位 </body>，拒绝修改。' }
        $candidateText = $clean.Snapshot.Text.Insert($bodyPos, $block)
        $candidateBytes = Convert-IndexTextToBytes -Text $candidateText -HasBom $baseline.HasBom
        try {
            $changed = Replace-EmbyIndexTransactional -Context $context -Source $current -CandidateBytes $candidateBytes -Baseline $baseline -ExpectPatch
        }
        catch {
            # Keep index.html and its referenced JS version consistent. Restore
            # the old JS only when index.html is still exactly our source and
            # no third party changed the just-synchronized JS.
            try {
                $indexAfterFailure = Get-FileSnapshot -Path $IndexHtml
                $liveAfterFailure = Get-FileSnapshot -Path $LiveJs
                if ($indexAfterFailure.Hash -eq $current.Hash -and $liveAfterFailure.Hash -eq $masterSnapshot.Hash) {
                    if ($liveBefore) {
                        Save-BytesTransactional -Path $LiveJs -Bytes $liveBefore.Bytes
                    }
                    else {
                        Remove-Item -LiteralPath $LiveJs -Force
                    }
                }
            }
            catch {
                throw ('WEB_PATCH_ASSET_ROLLBACK_FAILED：' + $_.Exception.Message)
            }
            throw
        }
        if ($changed) { Write-Host ('✅ Web Patch v' + $ExpectedWebCardVersion + ' 已通过事务校验安装。') -ForegroundColor Green }
        else { Write-Host ('✅ Web Patch v' + $ExpectedWebCardVersion + ' 已健康，NO_CHANGE。') -ForegroundColor Green }
    }
}

function Remove-WebPatch {
    if (-not (Test-Path -LiteralPath $IndexHtml -PathType Leaf)) { throw 'EMBY_INDEX_MISSING：不会自动使用可能过期的备份恢复。' }
    Invoke-WithWebPatchLock -Body {
        param($context)
        $current = Get-FileSnapshot -Path $IndexHtml
        $clean = Get-CleanIndexText -Snapshot $current
        if ($clean.HadPatch) {
            $baseline = Resolve-TrustedBaseline -Context $context -CurrentSnapshot $current -CleanResult $clean
            [void](Replace-EmbyIndexTransactional -Context $context -Source $current -CandidateBytes $baseline.Bytes -Baseline $baseline)
            $restored = Get-FileSnapshot -Path $IndexHtml
            if ($restored.Hash -ne $baseline.Hash) { throw 'RESTORE_VERIFY_FAILED：恢复结果与原版备份不是字节级一致。' }
        }
        foreach ($path in @(
            $LiveJs, $DataFile, $RuntimeStateFile, $IndexSummaryFile, $Worker, $MasterJs, $StateFile, $CacheFile,
            $RootDiscoveryFile, $RootStateFile, $XmlErrorFile, $CatalogFile
        )) {
            Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
        }
        Write-Host '✅ 已恢复可信原版 index.html 并移除 Web Card/派生索引；长期备份已保留。' -ForegroundColor Green
    }
}

function Get-CanonicalTagValue {
    param([string]$Value)
    $text = [System.Net.WebUtility]::HtmlDecode([string]$Value).Replace([char]0x00a0, ' ')
    $text = ([regex]::Replace($text, '\s+', ' ')).Trim()
    $text = [regex]::Replace($text, '\s*:\s*', ':').ToLowerInvariant()
    $ratio = [regex]::Match($text, '^(\d+(?:\.\d+)?):(\d+)(?:\s*\(.*\))?$')
    if ($ratio.Success) { return $ratio.Groups[1].Value + ':' + $ratio.Groups[2].Value }
    return $text
}

function Get-TechObject {
    param([string]$NfoPath)

    # Deliberately let XML read/parse errors bubble to the caller. A transient
    # partial write must NOT be cached as "this NFO has no technical specs";
    # the next scheduled pass should retry it.
    $xml = Read-XmlDocument -Path $NfoPath
    $root = $xml.DocumentElement

    if ($null -eq $root) {
        return $null
    }

    $rootName = ([string]$root.Name).ToLowerInvariant()
    $kind = $null

    if ($rootName -eq 'movie') {
        $kind = 'Movie'
    }
    elseif ($rootName -eq 'tvshow') {
        $kind = 'Series'
    }
    elseif ($rootName -eq 'episodedetails') {
        $kind = 'Episode'
    }
    elseif ($rootName -eq 'season') {
        $kind = 'Season'
    }
    else {
        return $null
    }

    $tech = $null

    foreach ($node in @($root.SelectNodes('./technicalspecs'))) {
        if (([string]$node.GetAttribute('source')).Trim() -ieq 'IMDb') {
            $tech = $node
        }
    }

    $imdb = ''
    if ($null -ne $tech) {
        $imdb = ([string]$tech.GetAttribute('imdbid')).Trim()
    }

    if (-not $imdb) {
        foreach ($node in @($root.SelectNodes('./uniqueid'))) {
            if (([string]$node.GetAttribute('type')).Trim() -ieq 'imdb') {
                $candidate = ([string]$node.InnerText).Trim()

                if ($candidate) {
                    $imdb = $candidate
                    break
                }
            }
        }
    }

    if ($imdb -notmatch '^tt\d{5,12}$') { $imdb = '' }

    $sections = [ordered]@{}

    foreach ($section in @($(if ($null -ne $tech) { $tech.SelectNodes('./section') }))) {
        $name = ([string]$section.GetAttribute('name')).Trim()

        if (-not $name) {
            continue
        }

        $values = New-Object System.Collections.ArrayList

        foreach ($item in @($section.SelectNodes('./item'))) {
            $value = ([string]$item.InnerText).Trim()

            if ($value -and -not $values.Contains($value)) {
                [void]$values.Add($value)
            }
        }

        if ($values.Count -gt 0) {
            $sections[$name] = @($values)
        }
    }

    function Direct-Text([string]$Name) {
        $node = $root.SelectSingleNode('./' + $Name)
        if ($null -eq $node) { return '' }
        return ([string]$node.InnerText).Trim()
    }

    # Ownership is display-only on Windows and comes exclusively from the
    # embedded NFO manifest. Never guess ownership from a tag's wording.
    $ownershipEntries = New-Object System.Collections.ArrayList
    if ($null -ne $tech) {
        foreach ($manifest in @($tech.SelectNodes('./manualtags'))) {
            if (([string]$manifest.GetAttribute('owner')).Trim() -ine 'IMDb Tech Manager') { continue }
            foreach ($entry in @($manifest.SelectNodes('./tag'))) {
                $value = ([string]$entry.InnerText).Trim()
                if ($value) {
                    [void]$ownershipEntries.Add([ordered]@{
                        value = $value; ownership = 'manual'; engine = ''
                    })
                }
            }
        }
        foreach ($manifest in @($tech.SelectNodes('./generatedtags'))) {
            if (([string]$manifest.GetAttribute('owner')).Trim() -ine 'IMDb Tech Manager') { continue }
            $engine = ([string]$manifest.GetAttribute('engine')).Trim().ToLowerInvariant()
            foreach ($entry in @($manifest.SelectNodes('./tag'))) {
                $value = ([string]$entry.InnerText).Trim()
                if ($value) {
                    [void]$ownershipEntries.Add([ordered]@{
                        value = $value; ownership = 'generated'; engine = $engine
                    })
                }
            }
        }
    }

    $usedOwnership = New-Object 'System.Collections.Generic.HashSet[int]'
    $tags = New-Object System.Collections.ArrayList
    foreach ($node in @($root.SelectNodes('./tag'))) {
        $value = ([string]$node.InnerText).Trim()
        if (-not $value) { continue }
        $ownership = 'external'
        $engine = ''
        $key = Get-CanonicalTagValue -Value $value
        for ($i = 0; $i -lt $ownershipEntries.Count; $i++) {
            if ($usedOwnership.Contains($i)) { continue }
            $candidate = $ownershipEntries[$i]
            if ((Get-CanonicalTagValue -Value ([string]$candidate.value)) -eq $key) {
                $ownership = [string]$candidate.ownership
                $engine = [string]$candidate.engine
                [void]$usedOwnership.Add($i)
                break
            }
        }
        [void]$tags.Add([ordered]@{
            value = $value
            ownership = $ownership
            engine = $engine
        })
    }

    return [pscustomobject]@{
        imdb = $imdb
        type = $kind
        title = Direct-Text -Name 'title'
        originalTitle = Direct-Text -Name 'originaltitle'
        showTitle = Direct-Text -Name 'showtitle'
        year = Direct-Text -Name 'year'
        season = Direct-Text -Name 'season'
        episode = Direct-Text -Name 'episode'
        tags = @($tags)
        hasTechnicalSpecs = ($sections.Count -gt 0)
        specs = $sections
    }
}

function Merge-SpecObject {
    param(
        [System.Collections.IDictionary]$Target,
        $Source
    )

    foreach ($entry in @(Get-MapEntries -Object $Source)) {
        $field = [string]$entry.Name

        if (-not $field) {
            continue
        }

        if (-not $Target.Contains($field)) {
            $Target[$field] = New-Object System.Collections.ArrayList
        }

        foreach ($value in @($entry.Value)) {
            $textValue = ([string]$value).Trim()

            if (
                $textValue -and
                -not $Target[$field].Contains($textValue)
            ) {
                [void]$Target[$field].Add($textValue)
            }
        }
    }
}

function Assert-FullRebuildRootsOnline {
    $libraries = @(Get-SelectedMediaLibraries | Where-Object { $_.Included })
    if ($libraries.Count -eq 0) { throw '没有已启用的媒体目录。' }
    $offline = @($libraries | Where-Object { -not (Test-Path -LiteralPath ([string]$_.Path) -PathType Container) })
    if ($offline.Count -gt 0) {
        $paths = @($offline | ForEach-Object { [string]$_.Path }) -join '；'
        throw ('FULL_REBUILD_OFFLINE_ROOT：完整重建已中止，不会清空旧缓存。请先恢复离线目录：' + $paths)
    }
}

function Update-TechIndex {
    param([switch]$SkipIfBusy, [switch]$ForceReparse)

    New-Item -ItemType Directory -Force -Path $CustomDir | Out-Null

    $mutex = New-Object System.Threading.Mutex(
        $false,
        'Local\EmbyTechnicalSpecsIndexCurrent'
    )

    $acquired = $false

    try {
        if ($SkipIfBusy) {
            $acquired = $mutex.WaitOne(0)

            if (-not $acquired) {
                return $null
            }
        }
        else {
            $acquired = $mutex.WaitOne([TimeSpan]::FromMinutes(5))

            if (-not $acquired) {
                throw '等待技术规格索引锁超时。'
            }
        }

        $libraries = @(Get-SelectedMediaLibraries)
        $allIncludedLibraries = @($libraries | Where-Object { $_.Included })
        $libraryRoots = @($allIncludedLibraries | ForEach-Object { [string]$_.Path } | Sort-Object -Unique)
        $includedLibraries = @($allIncludedLibraries)
        if ($OnlyRoot) {
            $requestedRoot = [System.IO.Path]::GetFullPath($OnlyRoot).TrimEnd('\')
            $includedLibraries = @(
                $allIncludedLibraries | Where-Object {
                    [string]::Equals(
                        [System.IO.Path]::GetFullPath([string]$_.Path).TrimEnd('\'),
                        $requestedRoot,
                        [System.StringComparison]::OrdinalIgnoreCase
                    )
                }
            )
            if ($includedLibraries.Count -ne 1) {
                throw ('目录级扫描目标不在已启用媒体目录中：' + $OnlyRoot)
            }
            Write-Host ('本轮只扫描此目录：' + [string]$includedLibraries[0].Path) -ForegroundColor Cyan
        }

        if ($libraryRoots.Count -eq 0) {
            throw '没有识别出任何电影/电视剧媒体库物理路径。'
        }

        $previousRoots = @(Load-RootState)
        $state = Load-JsonMap -Path $StateFile
        $cache = Load-JsonMap -Path $CacheFile
        $seen = @{}
        $successfullyScannedRoots = New-Object System.Collections.ArrayList
        $scanStats = [ordered]@{
            onlineRootsScanned = 0
            nfoSeen = 0
            nfoReparsed = 0
            technicalSpecsFound = 0
            webEligibleSpecsFound = 0
            episodeSpecsExcludedFromWeb = 0
            xmlReadErrors = 0
        }
        $xmlErrorRows = New-Object System.Collections.ArrayList

        foreach ($library in $includedLibraries) {
            $libraryRoot = ([string]$library.Path).Trim()

            if (-not $libraryRoot) {
                continue
            }

            if (-not (Test-Path -LiteralPath $libraryRoot -PathType Container)) {
                # Configured but temporarily offline/unmounted: retain its old
                # cache instead of interpreting this as a deletion.
                continue
            }

            $scanStats.onlineRootsScanned++
            $scanErrors = @()

            try {
                $nfoFiles = @(
                    Get-ChildItem `
                        -LiteralPath $libraryRoot `
                        -Recurse `
                        -File `
                        -Filter '*.nfo' `
                        -ErrorAction SilentlyContinue `
                        -ErrorVariable +scanErrors
                )
            }
            catch {
                $scanErrors += $_
                $nfoFiles = @()
            }

            foreach ($file in $nfoFiles) {
                $scanStats.nfoSeen++
                $path = $file.FullName
                $seen[$path] = $true
                $stamp = (
                    $ParserCacheVersion + ':' +
                    [string]$file.Length + ':' +
                    [string]$file.LastWriteTimeUtc.Ticks
                )

                $unchanged = $false

                if (
                    -not $ForceReparse -and
                    $state.ContainsKey($path) -and
                    $cache.ContainsKey($path)
                ) {
                    $unchanged = ([string]$state[$path] -eq $stamp)
                }

                if (-not $unchanged) {
                    $scanStats.nfoReparsed++
                    $parseSucceeded = $false
                    $obj = $null

                    try {
                        $obj = Get-TechObject -NfoPath $path
                        $parseSucceeded = $true
                    }
                    catch {
                        $scanStats.xmlReadErrors++
                        [void]$xmlErrorRows.Add(
                            [ordered]@{
                                path = $path
                                stamp = $stamp
                                error = $_.Exception.Message
                            }
                        )
                        # Do not update state/cache on a transient malformed or
                        # concurrently-written XML file. It will retry next run.
                    }

                    if ($parseSucceeded) {
                        if ($null -eq $obj) {
                            if ($cache.ContainsKey($path)) {
                                $cache.Remove($path)
                            }
                        }
                        else {
                            $cache[$path] = $obj
                        }

                        $state[$path] = $stamp
                    }
                }
            }

            # Only purge files that disappeared when traversal completed
            # without filesystem errors. A permission/network error must never
            # wipe valid cached metadata.
            if ($scanErrors.Count -eq 0) {
                [void]$successfullyScannedRoots.Add($libraryRoot)
            }
        }

        foreach ($path in @($state.Keys)) {
            $remove = $false

            foreach ($root in @($successfullyScannedRoots)) {
                if (
                    (Test-PathWithinRoot -Path $path -Root ([string]$root)) -and
                    -not $seen.ContainsKey($path)
                ) {
                    $remove = $true
                    break
                }
            }

            if ($remove) {
                $state.Remove($path)

                if ($cache.ContainsKey($path)) {
                    $cache.Remove($path)
                }
            }
        }

        # If a physical path was actually removed from Emby's configuration,
        # purge its old entries. Nested/overlapping paths are preserved when
        # they are still covered by any currently configured root.
        foreach ($oldRoot in $previousRoots) {
            $stillConfigured = $false

            foreach ($currentRoot in $libraryRoots) {
                if ([string]::Equals(
                    [string]$oldRoot,
                    [string]$currentRoot,
                    [System.StringComparison]::OrdinalIgnoreCase
                )) {
                    $stillConfigured = $true
                    break
                }
            }

            if ($stillConfigured) {
                continue
            }

            foreach ($path in @($state.Keys)) {
                if (
                    (Test-PathWithinRoot -Path $path -Root ([string]$oldRoot)) -and
                    -not (Test-PathWithinAnyRoot -Path $path -Roots $libraryRoots)
                ) {
                    $state.Remove($path)

                    if ($cache.ContainsKey($path)) {
                        $cache.Remove($path)
                    }
                }
            }
        }

        $mergedItems = [ordered]@{}
        # Public, path-free type hints let the Emby card recover when ApiClient
        # initializes late. Only card-eligible Movie/Series records enter this
        # map; Episode/Season pages therefore remain fail-closed.
        $itemTypes = [ordered]@{}
        $catalogRows = New-Object System.Collections.ArrayList
        $xmlErrorByPath = @{}
        $cataloguedPaths = @{}
        foreach ($xmlError in @($xmlErrorRows)) {
            $xmlErrorByPath[[string]$xmlError.path] = [string]$xmlError.error
        }

        foreach ($entry in @($cache.GetEnumerator() | Sort-Object Name)) {
            $obj = $entry.Value

            if ($null -eq $obj) {
                continue
            }

            # The Manager catalog lists every parsed NFO (including episodes)
            # so the GUI search works; the Web card index below is narrower.
            $objImdb = ([string]$obj.imdb).Trim()
            $rowKind = ''
            $rowRoot = ''
            $rowPath = [string]$entry.Name
            foreach ($lib in $allIncludedLibraries) {
                if (Test-PathWithinRoot -Path $rowPath -Root ([string]$lib.Path)) {
                    $rowKind = [string]$lib.Kind
                    $rowRoot = [string]$lib.Path
                    break
                }
            }
            $seriesTitle = ([string]$obj.showTitle).Trim()
            if (([string]$obj.type) -eq 'Series') {
                $seriesTitle = ([string]$obj.title).Trim()
            }
            elseif (-not $seriesTitle -and $rowRoot) {
                $lookupDir = Split-Path -Parent $rowPath
                while ($lookupDir -and (Test-PathWithinRoot -Path $lookupDir -Root $rowRoot)) {
                    $seriesNfoPath = Join-Path $lookupDir 'tvshow.nfo'
                    if ($cache.ContainsKey($seriesNfoPath)) {
                        $seriesObj = $cache[$seriesNfoPath]
                        if ($seriesObj -and ([string]$seriesObj.type) -eq 'Series') {
                            $seriesTitle = ([string]$seriesObj.title).Trim()
                            break
                        }
                    }
                    $parentDir = Split-Path -Parent $lookupDir
                    if (-not $parentDir -or $parentDir -eq $lookupDir) { break }
                    $lookupDir = $parentDir
                }
            }
            $rowError = ''
            if ($xmlErrorByPath.ContainsKey($rowPath)) {
                $rowError = [string]$xmlErrorByPath[$rowPath]
            }
            [void]$catalogRows.Add(
                [ordered]@{
                    path = $rowPath
                    type = [string]$obj.type
                    title = [string]$obj.title
                    originalTitle = [string]$obj.originalTitle
                    showTitle = [string]$obj.showTitle
                    seriesTitle = $seriesTitle
                    year = [string]$obj.year
                    season = [string]$obj.season
                    episode = [string]$obj.episode
                    imdb = $objImdb
                    libraryKind = $rowKind
                    hasTechnicalSpecs = [bool]$obj.hasTechnicalSpecs
                    tagCount = @($obj.tags).Count
                    tags = @($obj.tags)
                    specs = $obj.specs
                    error = $rowError
                }
            )
            $cataloguedPaths[$rowPath] = $true

            if ([bool]$obj.hasTechnicalSpecs) {
                $scanStats.technicalSpecsFound++
            }

            if (-not $obj.imdb -or -not $obj.specs) {
                continue
            }

            $imdb = ([string]$obj.imdb).Trim()

            if ($imdb -notmatch '^tt\d{5,12}$') {
                continue
            }

            # TV Technical Specifications are a programme/Series-level card.
            # Episode/Season NFOs may contain inherited or legacy specs, but
            # neither kind is ever exposed to the public Web Card index.
            if (([string]$obj.type) -eq 'Episode' -or ([string]$obj.type) -eq 'Season') {
                $scanStats.episodeSpecsExcludedFromWeb++
                continue
            }
            $scanStats.webEligibleSpecsFound++

            if (-not $itemTypes.Contains($imdb) -or ([string]$obj.type) -eq 'Series') {
                $itemTypes[$imdb] = [string]$obj.type
            }

            if (-not $mergedItems.Contains($imdb)) {
                $mergedItems[$imdb] = [ordered]@{}
            }

            Merge-SpecObject `
                -Target $mergedItems[$imdb] `
                -Source $obj.specs
        }

        # A malformed or concurrently-written NFO must remain visible in the
        # read-only Manager instead of disappearing into an unexplained empty
        # state. If a last-valid cached row exists it was annotated above;
        # otherwise add an explicit error placeholder for this exact path.
        foreach ($xmlError in @($xmlErrorRows)) {
            $errorPath = [string]$xmlError.path
            if ($cataloguedPaths.ContainsKey($errorPath)) { continue }
            $rowKind = ''
            foreach ($lib in $allIncludedLibraries) {
                if (Test-PathWithinRoot -Path $errorPath -Root ([string]$lib.Path)) {
                    $rowKind = [string]$lib.Kind
                    break
                }
            }
            [void]$catalogRows.Add(
                [ordered]@{
                    path = $errorPath
                    type = 'InvalidNFO'
                    title = [System.IO.Path]::GetFileNameWithoutExtension($errorPath)
                    originalTitle = ''
                    showTitle = ''
                    seriesTitle = ''
                    year = ''
                    season = ''
                    episode = ''
                    imdb = ''
                    libraryKind = $rowKind
                    hasTechnicalSpecs = $false
                    tagCount = 0
                    tags = @()
                    specs = [ordered]@{}
                    error = [string]$xmlError.error
                }
            )
        }

        $items = [ordered]@{}

        foreach ($imdb in $mergedItems.Keys) {
            $fieldMap = [ordered]@{}

            foreach ($field in $mergedItems[$imdb].Keys) {
                $fieldMap[$field] = @($mergedItems[$imdb][$field])
            }

            $items[$imdb] = $fieldMap
        }

        $outputLibraries = @(
            $allIncludedLibraries | ForEach-Object {
                [ordered]@{
                    name = [string]$_.Name
                    path = [string]$_.Path
                    kind = [string]$_.Kind
                    online = [bool]$_.Online
                    evidence = [string]$_.Evidence
                }
            }
        )

        $output = [ordered]@{
            version = 5
            generatedAt = (Get-Date).ToUniversalTime().ToString('o')
            libraryRoots = @($libraryRoots)
            libraries = @($outputLibraries)
            indexedTitles = $items.Count
            catalogCount = $catalogRows.Count
            scanStats = [pscustomobject]$scanStats
            items = $items
        }

        # Cache must reach disk before its freshness stamps. If power is lost
        # between these writes, an old stamp forces a harmless reparse; the
        # opposite order could incorrectly bless stale cached content.
        Save-JsonAtomic -Object $cache -Path $CacheFile -Depth 20
        Save-JsonAtomic -Object $state -Path $StateFile -Depth 6
        Save-RootState -Roots $libraryRoots
        Save-JsonAtomic -Object ([ordered]@{
            generatedAt = (Get-Date).ToUniversalTime().ToString('o')
            count = $xmlErrorRows.Count
            errors = @($xmlErrorRows)
        }) -Path $XmlErrorFile -Depth 8
        Save-JsonAtomic -Object ([ordered]@{
            generatedAt = (Get-Date).ToUniversalTime().ToString('o')
            count = $catalogRows.Count
            items = @($catalogRows)
        }) -Path $CatalogFile -Depth 20
        # The file under dashboard-ui is fetched by unauthenticated Emby Web
        # clients. Keep it limited to the card lookup; never publish local
        # filesystem paths, library roots, scan errors, or Manager metadata.
        Save-JsonAtomic -Object ([ordered]@{
            version = 7
            generatedAt = $output.generatedAt
            items = $items
            itemTypes = $itemTypes
        }) -Path $DataFile -Depth 24
        Save-JsonAtomic -Object $output -Path $IndexSummaryFile -Depth 24

        return $output
    }
    finally {
        if ($acquired) {
            try {
                $mutex.ReleaseMutex()
            }
            catch {}
        }

        $mutex.Dispose()
    }
}

function Show-IndexStatus {
    if (-not (Test-Path -LiteralPath $IndexSummaryFile -PathType Leaf)) {
        Write-Host '❌ Manager 索引摘要尚未生成。' -ForegroundColor Red
        return
    }
    try {
        $data = Read-Utf8Text -Path $IndexSummaryFile | ConvertFrom-Json
        $count = @($data.items.PSObject.Properties).Count
        Write-Host ('✅ 当前技术规格索引包含 ' + $count + ' 个 IMDb 条目。') -ForegroundColor Green
        if ($data.scanStats) {
            Write-Host (
                '扫描统计：roots=' + $data.scanStats.onlineRootsScanned +
                ', NFO=' + $data.scanStats.nfoSeen +
                ', reparsed=' + $data.scanStats.nfoReparsed +
                ', tech=' + $data.scanStats.technicalSpecsFound +
                ', webEligible=' + $data.scanStats.webEligibleSpecsFound +
                ', episodeWebSkipped=' + $data.scanStats.episodeSpecsExcludedFromWeb +
                ', xmlErrors=' + $data.scanStats.xmlReadErrors
            )
        }
        if ($data.libraryRoots) {
            Write-Host ''
            Write-Host '自动发现的 Emby 媒体库：' -ForegroundColor Cyan
            if ($data.libraries) {
                foreach ($library in @($data.libraries)) {
                    $onlineText = if ($library.online) { 'online' } else { 'offline' }
                    Write-Host ('  [' + $library.kind + '] ' + $library.path + '  ' + $onlineText)
                }
            }
            else {
                foreach ($root in @($data.libraryRoots)) { Write-Host ('  ' + $root) }
            }
        }
        Write-Host ''
        Write-Host 'Web 前端：' -ForegroundColor Cyan
        $htmlOk = $false
        $jsOk = $false
        try {
            $htmlText = Read-Utf8Text -Path $IndexHtml
            $htmlOk = (
                $htmlText -match ('technical-specs-card\.js\?v=' + [regex]::Escape($ExpectedWebCardVersion)) -and
                [regex]::Matches($htmlText, [regex]::Escape($PatchBegin)).Count -eq 1 -and
                [regex]::Matches($htmlText, [regex]::Escape($PatchEnd)).Count -eq 1
            )
        }
        catch {}
        try {
            $jsText = Read-Utf8Text -Path $LiveJs
            $jsOk = ($jsText -match ('WEB_CARD_VERSION\s*=\s*["'']' + [regex]::Escape($ExpectedWebCardVersion) + '["'']'))
        }
        catch {}
        if ($htmlOk -and $jsOk) {
            Write-Host ('  ✅ dashboard-ui 注入与 v' + $ExpectedWebCardVersion + ' JS 均正常。') -ForegroundColor Green
        }
        else {
            Write-Host '  ❌ dashboard-ui 注入或前端 JS 版本不正确。' -ForegroundColor Red
        }

        if ($data.scanStats -and [int]$data.scanStats.xmlReadErrors -gt 0) {
            Write-Host ('  ℹ️ XML 错误明细：' + $XmlErrorFile) -ForegroundColor Yellow
        }
    }
    catch { Write-Host ('❌ 读取 technical-specs-data.json 失败：' + $_.Exception.Message) -ForegroundColor Red }
}

if ($DiscoverOnly) {
    Write-Host ''
    Write-Host '=== Emby 电影 / 电视剧物理路径自动发现测试（只读） ===' -ForegroundColor Cyan
    Write-Host '原则：按 library.db 真实父子关系逐个物理根判断；绝不合并公共父目录。'
    Write-Host ''

    $libraries = @(Get-EmbyMediaLibraries)
    $included = @($libraries | Where-Object { $_.Included })
    $ignored = @($libraries | Where-Object { -not $_.Included })

    if ($included.Count -eq 0) {
        Write-Host '❌ 没有识别出电影/电视剧物理路径。' -ForegroundColor Red
    }
    else {
        Write-Host ('✅ 识别出 ' + $included.Count + ' 个电影/电视剧物理路径：') -ForegroundColor Green
        Write-Host ''

        foreach ($library in $included) {
            $status = if ($library.Online) { '在线' } else { '离线/不可访问' }
            Write-Host ('[' + $library.Kind + '] ' + $library.Path) -ForegroundColor Yellow
            Write-Host ('  Emby 节点：' + $library.Name + '  (Id=' + $library.Id + ', ParentId=' + $library.ParentId + ')')
            Write-Host ('  状态：' + $status)
            Write-Host ('  证据：' + $library.Evidence)
            Write-Host ''
        }
    }

    if ($ignored.Count -gt 0) {
        Write-Host '以下物理根已识别，但会排除：' -ForegroundColor DarkGray

        foreach ($library in $ignored) {
            Write-Host ('  [Ignored] ' + $library.Path + '  (' + $library.Evidence + ')') -ForegroundColor DarkGray
        }

        Write-Host ''
    }

    Write-Host '支持：多个电影库、多个电视剧库、同一虚拟库多个物理路径、电影/电视剧混合库。'
    Write-Host '离线路径不会导致旧索引被清空；音乐库和普通视频库不会进入技术规格扫描。'
    Write-Host ''
    Write-Host '确认这里列出的路径正确后，再不带 -DiscoverOnly 运行同一文件正式安装。'

    if ($included.Count -eq 0) {
        exit 2
    }

    exit 0
}

if ($DisableIntegration) { Remove-WebPatch; exit 0 }
if ($RepairWebOnly) {
    Sync-ManagerFrontendAsset
    Install-WebPatch
    Write-Host '✅ Web Card 已通过锁、CAS、日志与原子替换完成显式修复。'
    exit 0
}
if ($IndexOnly) {
    # Safety boundary: background/index-only runs read NFOs and update only
    # Manager-owned derived JSON. They never touch Emby's index.html or JS.
    [void](Update-TechIndex -SkipIfBusy)
    exit 0
}
if ($RebuildIndexOnly) {
    Assert-FullRebuildRootsOnline
    $rebuilt = Update-TechIndex -ForceReparse
    Write-Host ('✅ 只读 NFO 索引已完整重建，目录 ' + $rebuilt.catalogCount + ' 项；Emby index.html 未修改。') -ForegroundColor Green
    exit 0
}
if ($CheckOnly) { Show-IndexStatus; exit 0 }

if (-not (Test-Path -LiteralPath $IndexHtml -PathType Leaf)) { throw "找不到 Emby Web：$IndexHtml" }
New-Item -ItemType Directory -Force -Path $CustomDir | Out-Null
Sync-ManagerFrontendAsset

# Stop/remove an older scheduled copy before the first full rebuild, avoiding
# Earlier workers racing with this installer.
try {
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
}
catch {}

try {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
}
catch {}
# The current frontend asset is shipped as engine/technical-specs-card.js and
# was already synchronized above. Do not embed/write a historical JS snapshot.
Assert-WebCardVersion -Path $MasterJs
Save-BytesTransactional -Path $Worker -Bytes ([System.IO.File]::ReadAllBytes($PSCommandPath))

$tokens = $null
$parseErrors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile(
    $Worker,
    [ref]$tokens,
    [ref]$parseErrors
)

if ($parseErrors.Count -gt 0) {
    foreach ($err in $parseErrors) {
        Write-Host ('❌ Worker 语法错误：' + $err.Message) -ForegroundColor Red
        Write-Host ('   Line ' + $err.Extent.StartLineNumber + ': ' + $err.Extent.Text) -ForegroundColor Red
    }

    throw '复制后的 Manager Worker 未通过 PowerShell Parser 校验。'
}

$sourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $PSCommandPath).Hash
$workerHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Worker).Hash

if ($sourceHash -ne $workerHash) {
    throw 'Worker 复制后 SHA256 不一致。'
}

# Clean legacy versioned artifacts. Current builds use stable filenames so
# upgrades do not leave a trail of legacy state files behind.
$legacyPatterns = @(
    'technical-specs-worker-v*.ps1',
    'state-v*.json',
    'items-cache-v*.json',
    'root-discovery-v*.json',
    'root-state-v*.json',
    'xml-errors-v*.json',
    'library-roots-v*.json'
)

foreach ($pattern in $legacyPatterns) {
    Get-ChildItem -LiteralPath $CustomDir -File -Filter $pattern -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
}

Remove-Item -LiteralPath (Join-Path $CustomDir 'tech-specs-indexer.ps1') -Force -ErrorAction SilentlyContinue

# ManagedInstall is a deliberate full rebuild. Force every NFO through the
# parser in memory after the online-root preflight. Keep all last-valid index
# artifacts on disk until their atomic replacements succeed.
Assert-FullRebuildRootsOnline

Write-Host ''
Write-Host '=== 1. 首次建立技术规格索引 ===' -ForegroundColor Cyan
$output = Update-TechIndex -ForceReparse
Write-Host ('✅ 首次索引完成，共 ' + $output.indexedTitles + ' 个 IMDb 条目。') -ForegroundColor Green
Write-Host (
    '扫描统计：roots=' + $output.scanStats.onlineRootsScanned +
    ', NFO=' + $output.scanStats.nfoSeen +
    ', reparsed=' + $output.scanStats.nfoReparsed +
    ', tech=' + $output.scanStats.technicalSpecsFound +
    ', webEligible=' + $output.scanStats.webEligibleSpecsFound +
    ', episodeWebSkipped=' + $output.scanStats.episodeSpecsExcludedFromWeb +
    ', xmlErrors=' + $output.scanStats.xmlReadErrors
)

# Product-level validation: the index JSON itself must be valid and contain
# an items object.  Do not hard-code a particular title/IMDb id; the user may
# legitimately remove the original test movie later.
$verifyData = Read-Utf8Text -Path $DataFile | ConvertFrom-Json
if ($null -eq $verifyData -or $null -eq $verifyData.items) {
    throw '首次索引输出无效：technical-specs-data.json 缺少 items。'
}

# Only after a complete, valid card index exists may the explicit install
# action touch Emby's dashboard entry. A failed NFO rebuild leaves index.html
# byte-for-byte unchanged.
Install-WebPatch

Write-Host ''
Write-Host '=== 2. 验证索引 ===' -ForegroundColor Cyan
Show-IndexStatus

Write-Host ''
Write-Host '=== 3. 后台调度由 Tech Card Manager 接管 ===' -ForegroundColor Cyan
try {
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
} catch {}
try {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
} catch {}
Write-Host '✅ 已确保旧的每分钟 PowerShell 计划任务不存在。' -ForegroundColor Green

Write-Host ''
Write-Host '✅ Tech Card Manager Windows 4.0.2 Portable 索引引擎启用完成。' -ForegroundColor Green
Write-Host ''
Write-Host 'Tech Card Manager 4.0.2 支持 Portable 数据目录、用户选择媒体目录、目录级扫描与只读 NFO 目录。'
Write-Host '安全边界：后台增量检查永不改动 Emby index.html；Web Card 只在用户点击安装/修复时执行事务化注入。'
Write-Host '支持多个电影库/电视剧库、同一虚拟库多个物理路径和混合库；离线根保留缓存，恢复后自动续扫。'
Write-Host '网页卡片：电影优先沿用真实视频卡片，ISO/BDMV 使用独立卡片；电视剧只在节目首页展示，季和单集页面不显示；页面切换后自动校验条目身份。'
Write-Host ''
Write-Host '下一步：浏览器 Ctrl+F5，然后分别打开一个普通视频文件和一个 ISO 电影确认技术规格卡。iOS 原生 Emby App 不加载服务器 dashboard-ui 自定义 JS，属于客户端限制；标准 Tag 仍正常可用。'
