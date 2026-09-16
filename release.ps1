# Phosphor — pubblica una release GitHub SEMPRE allineata a Cargo.toml.
#
# Il numero di versione viene preso da Cargo.toml (unica fonte di verità: il
# binario lo eredita via env!("CARGO_PKG_VERSION")). Lo script:
#   1. legge la versione da Cargo.toml          -> tag = vX.Y.Z
#   2. CONTROLLI: tree git pulito; il tag non esiste già (locale né remoto)
#   3. build pulita (path utente rimappati, come build-release.ps1)
#   4. CONTROLLO CHIAVE: il binario deve riportare ESATTAMENTE quella versione
#      (impedisce il disallineamento tag/release vs Cargo.toml che c'era prima)
#   5. aggiorna dist/, committa e pusha se cambiato
#   6. crea il tag vX.Y.Z e la release GitHub con quel numero
#
# Uso:
#   pwsh -File release.ps1            # esegue la release
#   pwsh -File release.ps1 -DryRun    # solo i controlli (build+verifica), niente push/tag/release
#
# Per rilasciare una nuova versione: bumpa `version` in Cargo.toml, committa,
# poi lancia questo script. Niente da ricordare a mano: il numero lo decide Cargo.toml.

param([switch]$DryRun)
$ErrorActionPreference = 'Stop'

# 1) versione da Cargo.toml (la riga `version = "..."` del blocco [package],
#    ancorata a inizio riga: le dipendenze hanno `version` a metà riga).
$m = Select-String -Path 'Cargo.toml' -Pattern '^version\s*=\s*"([^"]+)"'
if (-not $m) { throw "Non trovo la versione in Cargo.toml" }
$ver = $m.Matches[0].Groups[1].Value
$tag = "v$ver"
Write-Host "Cargo.toml dice: $ver  ->  tag $tag" -ForegroundColor Cyan

# 2) controlli preliminari
if (-not $DryRun -and (git status --porcelain)) {
    throw "Working tree non pulito: committa prima di rilasciare (lo stamp dev'essere su un commit)."
}
if (git tag --list $tag)                                  { throw "Il tag $tag esiste già in locale. Bumpa la versione in Cargo.toml." }
if (git ls-remote --tags origin "refs/tags/$tag")        { throw "Il tag remoto $tag esiste già." }

# 3) la suite deve essere verde PRIMA di costruire qualcosa da pubblicare.
# Mancava: si poteva rilasciare con i test rossi e non se ne accorgeva nessuno.
Write-Host "cargo test …" -ForegroundColor Cyan
cargo test
if ($LASTEXITCODE -ne 0) { throw "cargo test FALLITO: niente release." }

# 3b) build pulita (rimappa la home utente: niente username nei binari)
$prev = $env:RUSTFLAGS
$env:RUSTFLAGS = "--remap-path-prefix=$($env:USERPROFILE)=~"
try { cargo build --release --features adventure } finally { $env:RUSTFLAGS = $prev }

# 4) CONTROLLO CHIAVE: il binario riporta esattamente la versione di Cargo.toml?
$stamp = (& ".\target\release\phosphor.exe" --version | Out-String).Trim()
if ($stamp -notmatch ("v" + [regex]::Escape($ver) + "(\b|$)")) {
    throw "MISMATCH versione: il binario riporta '$stamp' ma Cargo.toml dice v$ver."
}
Write-Host "OK — binario allineato: $stamp" -ForegroundColor Green

# 4b) il selftest gira la TUI vera sullo store vero: e' l'unica prova che quello
# che stiamo per pubblicare si apre e risponde. Non lo eseguiva nessuno, e
# infatti conteneva un contratto vecchio di mesi senza che si vedesse.
Write-Host "selftest …" -ForegroundColor Cyan
$self = (& ".\target\release\phosphor.exe" --selftest 2>&1 | Out-String)
if ($self -notmatch "selftest TUI: OK") { throw "SELFTEST FALLITO:`n$self" }
Write-Host "OK — selftest passato" -ForegroundColor Green

if ($DryRun) {
    Write-Host "DryRun: tutti i controlli superati. Niente tag/release pubblicati." -ForegroundColor Yellow
    return
}

# 5) prepara i binari in dist/ SOLO per allegarli alla release. I .exe NON sono
#    tracciati (sono in .gitignore): il repo resta "da sorgente", gli eseguibili
#    vivono unicamente come asset della GitHub Release (li scarica install.ps1).
Get-Process phosphor, phosphor-adv -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300
New-Item -ItemType Directory -Force 'dist' | Out-Null
Copy-Item 'target\release\phosphor.exe'     'dist\phosphor.exe'     -Force
Copy-Item 'target\release\phosphor-adv.exe' 'dist\phosphor-adv.exe' -Force

# 6) checksum di integrità (formato sha256sum: "<hash>  <file>") — pubblicato con
#    la release e verificato da install.ps1. Stampa gli hash così puoi ancorarli
#    FUORI BANDA (note di release/commit firmato): un checksum co-localizzato da
#    solo non difende da un canale compromesso.
$assets = @('dist\phosphor.exe', 'dist\phosphor-adv.exe', 'install.ps1')
$sumsPath = 'dist\SHA256SUMS.txt'
$lines = foreach ($a in $assets) {
    $h = (Get-FileHash -Algorithm SHA256 -Path $a).Hash.ToLower()
    "{0}  {1}" -f $h, (Split-Path -Leaf $a)
}
Set-Content -Path $sumsPath -Value $lines -Encoding ascii
Write-Host "SHA256SUMS:" -ForegroundColor Cyan
$lines | ForEach-Object { Write-Host "  $_" }

# 7) tag + release con il numero di Cargo.toml
git tag $tag
git push origin $tag
gh release create $tag 'dist\phosphor.exe' 'dist\phosphor-adv.exe' 'install.ps1' 'dist\SHA256SUMS.txt' `
    --title "Phosphor $tag" --latest --generate-notes
Write-Host "Release $tag pubblicata e allineata a Cargo.toml (con SHA256SUMS)." -ForegroundColor Green
