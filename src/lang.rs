//! Due lingue, italiano e inglese, scelte a programma acceso.
//!
//! Niente registro di chiavi. Le due versioni di ogni frase stanno **una
//! accanto all'altra** nel punto in cui la frase serve:
//!
//! ```ignore
//! app.status = t!("scan completato", "scan complete").into();
//! ```
//!
//! È la scelta che regge meglio in un programma di questa taglia. Un file di
//! traduzioni separato sembra più ordinato finché non ci si lavora dentro: si
//! aggiunge una stringa e ci si dimentica dell'altra lingua, e il difetto si
//! vede solo a qualcuno che apre il programma in inglese, cioè mai a chi lo
//! scrive. Qui la coppia non può separarsi: è la stessa riga di codice.
//!
//! La lingua è un solo byte globale perché il disegno la interroga migliaia di
//! volte per fotogramma e passarla a ogni funzione avrebbe cambiato la firma di
//! mezzo programma per un dato che non cambia mai durante un disegno.

use std::sync::atomic::{AtomicU8, Ordering};

const IT: u8 = 0;
const EN: u8 = 1;

static CURRENT: AtomicU8 = AtomicU8::new(IT);

/// La lingua è l'inglese? Letta a ogni frase: `Relaxed` basta, non c'è nulla
/// da sincronizzare oltre al byte stesso.
pub fn is_en() -> bool {
    CURRENT.load(Ordering::Relaxed) == EN
}

pub fn set_en(en: bool) {
    CURRENT.store(if en { EN } else { IT }, Ordering::Relaxed);
}

/// `"it"` / `"en"`, come si scrive in `phosphor.json`.
pub fn code() -> &'static str {
    if is_en() {
        "en"
    } else {
        "it"
    }
}

/// Legge il codice dalla configurazione. Qualunque cosa che non sia `en`
/// vale italiano: una lingua che non conosciamo non è un errore da segnalare,
/// è semplicemente una che non abbiamo.
pub fn set_from_code(code: &str) {
    set_en(code.trim().eq_ignore_ascii_case("en"));
}

/// La frase nella lingua corrente. I due rami possono essere letterali o
/// `format!`: il macro non valuta quello che non serve.
#[macro_export]
macro_rules! t {
    ($it:expr, $en:expr $(,)?) => {
        if $crate::lang::is_en() { $en } else { $it }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pair_switches_together_and_unknown_codes_stay_italian() {
        set_en(false);
        assert!(!is_en());
        assert_eq!(code(), "it");
        assert_eq!(t!("ciao", "hello"), "ciao");

        set_from_code("en");
        assert!(is_en());
        assert_eq!(code(), "en");
        assert_eq!(t!("ciao", "hello"), "hello");
        // e funziona anche con stringhe costruite, non solo letterali
        let n = 3;
        assert_eq!(t!(format!("{n} righe"), format!("{n} rows")), "3 rows");

        set_from_code("EN");
        assert!(is_en(), "il codice non e' sensibile alle maiuscole");
        // Una lingua che non abbiamo non e' un errore: si resta in italiano.
        for unknown in ["fr", "", "  ", "italiano", "de"] {
            set_from_code(unknown);
            assert!(!is_en(), "«{unknown}» non deve accendere l'inglese");
        }
        set_en(false);
    }
}
