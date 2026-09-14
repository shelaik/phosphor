//! Count the turns where the user had to push back.
//!
//! Every other number on the Wrapped card flatters you: tokens spent, sessions
//! run, days in a row. This one does not — it counts the times you had to stop
//! the agent and say *no, not like that*. It is the most honest line on the
//! card, and the only one that says something about the collaboration rather
//! than the volume.
//!
//! It is a **heuristic over the user's own words**, not a judgement: a prompt
//! counts when it opens like a correction ("no,", "non è", "sbagliato",
//! "aspetta") or reports something broken ("non funziona", "dà errore"). It is
//! therefore an undercount of disagreements and an occasional overcount of
//! ordinary sentences, which is why the card labels it as an estimate. Matching
//! only the OPENING of a prompt keeps it that way: a long message that merely
//! mentions an error somewhere in the middle is not a correction.

/// Openings that mean "you got it wrong". Italian first — the user writes in
/// Italian — then the English equivalents, since a mixed-language history is
/// the normal case.
const OPENERS: [&str; 40] = [
    "no ", "no,", "no.", "nope", "non è", "non e'", "non va", "non funziona",
    "non ha funzionato", "non mi", "non ti", "non hai", "non devi", "non serve",
    "sbagliato", "errore", "aspetta", "fermo", "fermati", "anzi", "invece",
    "in realtà", "in realta'", "ma no", "però no", "pero' no", "ricontrolla",
    "hai sbagliato", "non quello", "non così", "non cosi'",
    // Aggiunte dopo averle viste nella history vera: dire "non ho capito" o
    // "non si vede nulla" e' correggere la rotta quanto dire "no".
    "non ho capito", "non si ", "non riesc", "non compare", "non vedo",
    "that's wrong", "not what i", "wrong,", "still not",
];

/// Phrases that report a failure wherever they appear in a short prompt: the
/// user is not disagreeing, but they are still correcting course.
const FAILURES: [&str; 10] = [
    "non funziona", "dà errore", "da errore", "si blocca", "va in crash",
    "doesn't work", "does not work", "still broken", "è rotto", "e' rotto",
];

/// Longest prompt still considered a correction when it merely CONTAINS a
/// failure phrase. A short "non funziona" is a correction; a three-page spec
/// that happens to mention an error is a task.
const SHORT: usize = 240;

/// True when this user prompt reads as a correction of what the agent just did.
pub fn is_correction(prompt: &str) -> bool {
    let t = prompt.trim_start().to_lowercase();
    if t.is_empty() {
        return false;
    }
    // Scaffolding — caveats, slash commands, injected blocks — is not the user
    // talking, so it can never be a correction.
    if t.starts_with('<') || t.starts_with('/') || t.starts_with("caveat:") {
        return false;
    }
    if OPENERS.iter().any(|o| t.starts_with(o)) {
        return true;
    }
    t.len() <= SHORT && FAILURES.iter().any(|f| t.contains(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_a_push_back() {
        for s in [
            "no, non era quello che intendevo",
            "No. rifallo",
            "non funziona, dà errore 500",
            "aspetta, hai sbagliato file",
            "anzi, lascia stare",
            "in realtà volevo il contrario",
            "that's wrong, try again",
            "  Non è quello che ti ho chiesto  ",
        ] {
            assert!(is_correction(s), "doveva contare: {s}");
        }
    }

    #[test]
    fn leaves_ordinary_work_alone() {
        for s in [
            "aggiungi un test per il parser",
            "ok, procedi",
            "/model",
            "<environment_context>x</environment_context>",
            "Caveat: the messages below were generated",
            "",
            // "nome" e "nota" iniziano per "no" ma non sono un rifiuto: il
            // confine e' lo spazio o la virgola dopo il "no"
            "nomina la variabile come vuoi",
            "nota che il file e' grosso",
        ] {
            assert!(!is_correction(s), "non doveva contare: {s}");
        }
    }

    #[test]
    fn a_long_spec_that_mentions_an_error_is_not_a_correction() {
        let mut long = String::from("implementa il modulo che gestisce i casi limite. ");
        while long.len() < SHORT + 50 {
            long.push_str("descrizione dettagliata del comportamento atteso. ");
        }
        long.push_str("quando non funziona deve loggare.");
        assert!(!is_correction(&long));
        // …la stessa frase, da sola, invece si'
        assert!(is_correction("quando non funziona deve loggare."));
    }
}
