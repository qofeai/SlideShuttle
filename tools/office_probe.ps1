# Windows PowerPoint evidence collection. Never modifies the supplied input.
# Requires desktop PowerPoint and the same fonts used for native rendering.
# Example: .\tools\office_probe.ps1 -InputPptx C:\demo.pptx -OutDir C:\probe
param(
    [Parameter(Mandatory=$true)][string]$InputPptx,
    [Parameter(Mandatory=$true)][string]$OutDir
)
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $InputPptx).Path
if (Test-Path -LiteralPath $OutDir) { throw 'Choose a new output directory.' }
$out = [IO.Path]::GetFullPath($OutDir)
[IO.Directory]::CreateDirectory($out) | Out-Null
$app = $null
$manifest = [ordered]@{ inputSha256=(Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash; cases=@() }
try {
    $app = New-Object -ComObject PowerPoint.Application
    $manifest.powerPointVersion = $app.Version
    $manifest.operatingSystem = [Environment]::OSVersion.VersionString
    Add-Type -AssemblyName System.Drawing
    $fonts = New-Object System.Drawing.Text.InstalledFontCollection
    $manifest.installedFontFamilies = @($fonts.Families | ForEach-Object { $_.Name })
    foreach ($case in @('save-only','edit-text','move','duplicate','delete')) {
        # ReadOnly true; WithWindow false; every variant is saved to a new path.
        $presentation = $app.Presentations.Open($source, -1, 0, 0)
        try {
            $slide = $presentation.Slides.Item(1)
            $shape = @($slide.Shapes | Where-Object { $_.HasTextFrame -eq -1 -and $_.TextFrame.HasText -eq -1 })[0]
            if ($null -eq $shape) { throw 'First slide needs a text object for this probe.' }
            switch ($case) {
                'edit-text' { $shape.TextFrame.TextRange.Text = 'Office 改稿 / edited text' }
                'move' { $shape.Left = $shape.Left + 18 }
                'duplicate' { $shape.Duplicate() | Out-Null }
                'delete' { $shape.Delete() }
            }
            $caseDir = Join-Path $out $case
            [IO.Directory]::CreateDirectory($caseDir) | Out-Null
            $file = Join-Path $caseDir 'saved.pptx'
            $presentation.SaveAs($file,24) # ppSaveAsOpenXMLPresentation
            $objects = @()
            foreach ($s in $presentation.Slides) {
                $s.Export((Join-Path $caseDir ('slide-{0:d3}.png' -f $s.SlideIndex)), 'PNG',1280,720)
                foreach ($o in $s.Shapes) {
                    $entry = [ordered]@{ page=$s.SlideIndex; id=$o.Id; name=$o.Name; type=[int]$o.Type; x=$o.Left; y=$o.Top; width=$o.Width; height=$o.Height }
                    if ($o.HasTextFrame -eq -1 -and $o.TextFrame.HasText -eq -1) {
                        $text=$o.TextFrame.TextRange
                        $entry.text=$text.Text; $entry.lines=$text.Lines().Count
                        $entry.boundHeight=$text.BoundHeight; $entry.boundWidth=$text.BoundWidth
                        $entry.font=$text.Font.Name; $entry.fontSize=$text.Font.Size
                    }
                    $objects += $entry
                }
            }
            $objects | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $caseDir 'objects.json') -Encoding utf8
            $manifest.cases += @{ name=$case; output=$file; sha256=(Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash }
        } finally { $presentation.Close() }
    }
    $manifest.completed = $true
} catch {
    $manifest.completed = $false
    $manifest.error = $_.Exception.Message
    throw
} finally {
    $manifest | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $out 'manifest.json') -Encoding utf8
    if ($null -ne $app) { $app.Quit(); [Runtime.InteropServices.Marshal]::ReleaseComObject($app) | Out-Null }
}
