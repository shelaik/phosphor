# Phosphor — installer leggero per Windows (PowerShell).
#
# Cosa fa: copia phosphor.exe (e, se presente, phosphor-adv.exe) in
#   %LOCALAPPDATA%\Phosphor, crea un collegamento sul Desktop e aggiunge la
#   cartella al PATH dell'utente (così puoi lanciare `phosphor` da qualsiasi
#   terminale). Niente diritti di amministratore, niente runtime: l'exe e'
#   autonomo.
#
# Uso:
#   - scarica phosphor.exe (e opzionale phosphor-adv.exe) in una cartella,
#     metti accanto questo install.ps1, poi tasto destro > "Esegui con PowerShell"
#   - oppure da terminale:  powershell -ExecutionPolicy Bypass -File install.ps1
#
# Disinstallare: cancella %LOCALAPPDATA%\Phosphor e il collegamento sul Desktop.

$ErrorActionPreference = 'Stop'
$src = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $env:LOCALAPPDATA 'Phosphor'

Write-Host "Phosphor — installazione in $dest" -ForegroundColor Green
New-Item -ItemType Directory -Force -Path $dest | Out-Null

# Integrità: se accanto c'è SHA256SUMS.txt (pubblicato con la release), verifica
# gli hash PRIMA di copiare; su mancata corrispondenza si annulla. Stampa comunque
# l'hash calcolato così puoi confrontarlo con quello nelle note di release
# (verifica fuori banda: un checksum co-localizzato da solo non basta).
$sumsFile = Join-Path $src 'SHA256SUMS.txt'
$sums = @{}
if (Test-Path $sumsFile) {
    foreach ($line in Get-Content $sumsFile) {
        if ($line -match '^\s*([0-9a-fA-F]{64})\s+\*?(.+?)\s*$') {
            $sums[$matches[2].ToLower()] = $matches[1].ToLower()
        }
    }
}
function Confirm-Hash($path, $name) {
    $h = (Get-FileHash -Algorithm SHA256 -Path $path).Hash.ToLower()
    Write-Host ("  SHA256 {0} = {1}" -f $name, $h)
    $key = $name.ToLower()
    if ($sums.ContainsKey($key)) {
        if ($sums[$key] -ne $h) {
            Write-Host "  [X] HASH NON CORRISPONDENTE per $name - installazione annullata." -ForegroundColor Red
            Write-Host "    atteso: $($sums[$key])" -ForegroundColor Red
            exit 1
        }
        Write-Host "  [OK] hash verificato ($name)" -ForegroundColor Green
    } else {
        Write-Host "  (nessun SHA256SUMS.txt: confronta l'hash sopra con quello nelle note di release)" -ForegroundColor Yellow
    }
}

$exe = Join-Path $src 'phosphor.exe'
if (-not (Test-Path $exe)) {
    Write-Host "Non trovo phosphor.exe accanto a questo script." -ForegroundColor Red
    Write-Host "Mettilo nella stessa cartella di install.ps1 e riprova." -ForegroundColor Red
    exit 1
}
Confirm-Hash $exe 'phosphor.exe'
Copy-Item $exe (Join-Path $dest 'phosphor.exe') -Force
Write-Host "  copiato phosphor.exe"

$adv = Join-Path $src 'phosphor-adv.exe'
if (Test-Path $adv) {
    Confirm-Hash $adv 'phosphor-adv.exe'
    Copy-Item $adv (Join-Path $dest 'phosphor-adv.exe') -Force
    Write-Host "  copiato phosphor-adv.exe (versione grafica)"
}

# Collegamento sul Desktop
$desktop = [Environment]::GetFolderPath('Desktop')
$lnk = Join-Path $desktop 'Phosphor.lnk'
$ws = New-Object -ComObject WScript.Shell
$sc = $ws.CreateShortcut($lnk)
$sc.TargetPath = Join-Path $dest 'phosphor.exe'
$sc.WorkingDirectory = $dest
$sc.Description = 'Phosphor — scanner delle sessioni di Claude Code'
$sc.Save()
Write-Host "  creato collegamento sul Desktop: Phosphor.lnk"

# Aggiunge al PATH dell'utente (idempotente)
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$dest*") {
    $newPath = if ([string]::IsNullOrEmpty($userPath)) { $dest } else { "$userPath;$dest" }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "  aggiunto al PATH utente (riapri il terminale per usare 'phosphor')"
} else {
    Write-Host "  PATH gia' configurato"
}

Write-Host ""
Write-Host "Fatto. Avvia dal Desktop (Phosphor) o digita 'phosphor' in un nuovo terminale." -ForegroundColor Green
Write-Host "Aiuto:  phosphor --help   ·   nell'app premi  ?  per la guida." -ForegroundColor Green
