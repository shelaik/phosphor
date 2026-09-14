# Phosphor — build di release "pulita" per la distribuzione.
#
# Perche': un binario Rust incorpora i path di compilazione (es. nei messaggi di
# panic, file!()/module_path!()). Quei path contengono la HOME dell'utente che
# compila (C:\Users\<tuonome>\...), cioe' il tuo username finisce nell'exe che
# distribuisci. Questo script rimappa la home a "~" cosi' il binario pubblico
# non incorpora il tuo username. NB: nessun dato di sessione e' MAI incluso —
# l'exe e' codice compilato, non legge le tue conversazioni a build-time.
#
# Usa $env:USERPROFILE (niente username hardcodato), quindi e' riproducibile su
# qualsiasi macchina. Produce target/release/phosphor.exe e phosphor-adv.exe.
#
# Uso:  powershell -ExecutionPolicy Bypass -File build-release.ps1

$ErrorActionPreference = 'Stop'
$prev = $env:RUSTFLAGS
# Rimappa la home utente (copre src del progetto, ~/.cargo e ~/.rustup).
$env:RUSTFLAGS = "--remap-path-prefix=$($env:USERPROFILE)=~"
try {
    cargo build --release --features adventure
} finally {
    $env:RUSTFLAGS = $prev
}
Write-Host ""
Write-Host "Build pulita: target/release/phosphor.exe + phosphor-adv.exe (path utente rimappati)." -ForegroundColor Green
