//! Text formatting pipeline for Whisper transcriptions.
//!
//! # Public API
//!
//! - [`format_text`] — Full cleanup pipeline. Removes fillers, fixes contractions, capitalizes
//!   sentences, ensures trailing punctuation. Use for most transcriptions.
//! - [`apply_replacements`] — Post-format whole-word substitution driven by the user's dictionary.
//!   Run this *after* `format_text` so replacements see the cleaned output.
//!
//! # Pipeline order (inside `format_text`)
//!
//! 1. `cleanup_false_start` — drop text before a restart marker (`— actually`, `— let me`, etc.)
//! 2. `remove_fillers` — strip filler words (`um`, `uh`, `you know`, pause-gated `like`)
//! 3. `collapse_repeated_phrases` — deduplicate consecutive repeated words/short phrases
//! 4. `remove_stutter_before_contraction` — drop single-letter stutter before its contraction form
//! 5. `capitalize_i_forms` — uppercase standalone `i` and `i'*` contractions
//! 6. `fix_contractions` — restore apostrophes in unambiguous contractions (`dont` → `don't`)
//! 7. `capitalize_sentences` — uppercase first letter after sentence-ending punctuation
//! 8. `ensure_trailing_punctuation` — append `.` if text doesn't already end with `.`, `!`, or `?`
//!
//! # `token_core()` convention
//!
//! Many pipeline passes strip leading/trailing non-alphanumeric characters (except `'`) and
//! lowercase the result before comparison.  This "core" string lets filler detection and
//! deduplication work correctly on tokens that carry trailing punctuation (e.g. `"like,"`, `"um."`).

use crate::config::ReplacementEntry;

// ── Public API ───────────────────────────────────────────────────────────────

/// Apply dictionary replacements after formatting.
/// Whole-word, case-insensitive match → replace with exact `to` spelling.
pub fn apply_replacements(text: &str, replacements: &[ReplacementEntry]) -> String {
    if replacements.is_empty() {
        return text.to_string();
    }

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = Vec::with_capacity(words.len());

    for word in &words {
        let stripped = word.trim_end_matches(|c: char| c == ',' || c == '.' || c == '!' || c == '?');
        let trailing = &word[stripped.len()..];

        let mut replaced = false;
        for entry in replacements {
            if stripped.eq_ignore_ascii_case(&entry.from) {
                out.push(format!("{}{trailing}", entry.to));
                replaced = true;
                break;
            }
        }
        if !replaced {
            out.push(word.to_string());
        }
    }

    out.join(" ")
}

pub fn format_text(input: &str) -> String {
    let cleaned = smart_cleanup(input);
    let text = cleaned.trim();
    if text.is_empty() {
        return String::new();
    }

    // Process word-by-word to handle "I" capitalization correctly
    let mut result = capitalize_i_forms(text);

    // Fix contractions without apostrophes (case-insensitive)
    let contraction_fixes: &[(&str, &str)] = &[
        ("dont", "don't"),
        ("cant", "can't"),
        ("wont", "won't"),
        ("didnt", "didn't"),
        ("doesnt", "doesn't"),
        ("isnt", "isn't"),
        ("wasnt", "wasn't"),
        ("werent", "weren't"),
        ("wouldnt", "wouldn't"),
        ("couldnt", "couldn't"),
        ("shouldnt", "shouldn't"),
        ("hasnt", "hasn't"),
        ("havent", "haven't"),
        ("hadnt", "hadn't"),
        ("youre", "you're"),
        ("theyre", "they're"),
        ("were", "we're"), // careful — also a real word, handled below
        ("thats", "that's"),
        ("whats", "what's"),
        ("heres", "here's"),
        ("theres", "there's"),
        ("lets", "let's"),
    ];

    for (from, to) in contraction_fixes {
        result = replace_whole_word_ci(&result, from, to);
    }

    // Capitalize first letter of each sentence
    result = capitalize_sentences(&result);

    // Ensure trailing punctuation
    if !result.ends_with(['.', '!', '?']) {
        result.push('.');
    }

    result
}

/// Replace "i", "i'm", "i'd", "i'll", "i've", "i'd" with capitalized "I" forms.
/// Handles: standalone "i", and all "i'" contractions regardless of case.
fn capitalize_i_forms(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = Vec::with_capacity(words.len());

    for word in words {
        let lower = word.to_lowercase();
        let fixed =
            match lower.trim_end_matches(|c: char| c == ',' || c == '.' || c == '!' || c == '?') {
                "i" => word.to_lowercase().replacen("i", "I", 1),
                w if w.starts_with("i'") || w.starts_with("i'") => {
                    // i'm, i'd, i'll, i've, i'd
                    let rest = &lower[1..];
                    format!("I{rest}")
                }
                _ => word.to_string(),
            };
        // Preserve trailing punctuation from original
        let trailing: String = word
            .chars()
            .rev()
            .take_while(|c| matches!(c, ',' | '.' | '!' | '?'))
            .collect();
        if !trailing.is_empty() && !fixed.ends_with(|c: char| matches!(c, ',' | '.' | '!' | '?')) {
            out.push(format!(
                "{fixed}{}",
                trailing.chars().rev().collect::<String>()
            ));
        } else {
            out.push(fixed);
        }
    }

    out.join(" ")
}

/// Case-insensitive whole-word replacement.
fn replace_whole_word_ci(text: &str, from: &str, to: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = Vec::with_capacity(words.len());

    for word in &words {
        let stripped =
            word.trim_end_matches(|c: char| c == ',' || c == '.' || c == '!' || c == '?');
        let trailing = &word[stripped.len()..];

        if stripped.eq_ignore_ascii_case(from) {
            // Don't replace "were" with "we're" — "were" is a real word
            // Only replace if it's clearly a contraction context
            if from == "were" {
                out.push(word.to_string());
            } else {
                out.push(format!("{to}{trailing}"));
            }
        } else {
            out.push(word.to_string());
        }
    }

    out.join(" ")
}

/// Capitalize first letter of the string and after sentence-ending punctuation.
fn capitalize_sentences(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut capitalize_next = true;

    for c in text.chars() {
        if capitalize_next && c.is_alphabetic() {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
            if matches!(c, '.' | '!' | '?') {
                capitalize_next = true;
            }
        }
    }

    result
}

// ── Pipeline Stages ──────────────────────────────────────────────────────────

fn smart_cleanup(input: &str) -> String {
    let mut text = collapse_whitespace(input.trim());
    text = cleanup_false_start(&text);

    let mut tokens = tokenize(&text);
    tokens = remove_fillers(tokens);
    tokens = collapse_repeated_phrases(tokens);
    tokens = remove_stutter_before_contraction(tokens);

    normalize_spacing(&tokens.join(" "))
}

fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace().map(ToString::to_string).collect()
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Cleanup Sub-passes ───────────────────────────────────────────────────────

fn cleanup_false_start(input: &str) -> String {
    for marker in ["—", "–", "--"] {
        if let Some(index) = input.rfind(marker) {
            let before = input[..index].trim();
            let after = input[index + marker.len()..]
                .trim_start_matches(|c: char| c == '-' || c == '—' || c == '–')
                .trim();

            if before.is_empty() || after.is_empty() {
                continue;
            }

            let lower_after = after.to_lowercase();
            let restart_marker = lower_after.starts_with("actually")
                || lower_after.starts_with("let me")
                || lower_after.starts_with("sorry")
                || lower_after.starts_with("i mean")
                || lower_after.starts_with("wait")
                || lower_after.starts_with("no ");

            if restart_marker || before.split_whitespace().count() <= 8 {
                return after.to_string();
            }
        }
    }

    input.to_string()
}

fn remove_fillers(tokens: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;

    while i < tokens.len() {
        if is_punctuation_only(&tokens[i]) {
            i += 1;
            continue;
        }

        let current = token_core(&tokens[i]);
        let next = tokens.get(i + 1).map(|t| token_core(t));
        let prev_has_pause = out.last().map_or(false, |t: &String| token_has_pause(t));

        if matches!(current.as_str(), "um" | "uh" | "hmm") {
            i += 1;
            continue;
        }

        if current == "uh" && next.as_deref() == Some("huh") {
            i += 2;
            continue;
        }

        if current == "you" && next.as_deref() == Some("know") {
            let phrase_has_pause = token_has_pause(&tokens[i + 1]);
            if prev_has_pause || phrase_has_pause {
                i += 2;
                continue;
            }
        }

        if current == "i" && next.as_deref() == Some("mean") {
            let phrase_has_pause = token_has_pause(&tokens[i + 1]);
            if prev_has_pause || phrase_has_pause {
                i += 2;
                continue;
            }
        }

        if current == "like" {
            let this_has_pause = token_has_pause(&tokens[i]);
            if prev_has_pause || this_has_pause {
                i += 1;
                continue;
            }
        }

        out.push(tokens[i].clone());
        i += 1;
    }

    out
}

fn collapse_repeated_phrases(tokens: Vec<String>) -> Vec<String> {
    if tokens.len() < 2 {
        return tokens;
    }

    let cores: Vec<String> = tokens.iter().map(|token| token_core(token)).collect();
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;

    while i < tokens.len() {
        let remaining = tokens.len() - i;
        let max_phrase_len = usize::min(3, remaining / 2);
        let mut matched_len = 0;

        for phrase_len in (1..=max_phrase_len).rev() {
            if repeated_phrase_at(&cores, i, phrase_len) {
                matched_len = phrase_len;
                break;
            }
        }

        if matched_len == 0 {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        }

        let phrase = cores[i..i + matched_len].to_vec();
        out.extend(tokens[i..i + matched_len].iter().cloned());
        i += matched_len;

        while i + matched_len <= tokens.len() && cores[i..i + matched_len] == phrase[..] {
            i += matched_len;
        }
    }

    out
}

fn repeated_phrase_at(cores: &[String], start: usize, phrase_len: usize) -> bool {
    if phrase_len == 0 || start + phrase_len * 2 > cores.len() {
        return false;
    }

    let left = &cores[start..start + phrase_len];
    let right = &cores[start + phrase_len..start + phrase_len * 2];

    if left.iter().any(|core| core.is_empty()) || right.iter().any(|core| core.is_empty()) {
        return false;
    }

    left == right
}

fn remove_stutter_before_contraction(tokens: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(tokens.len());

    for (index, token) in tokens.iter().enumerate() {
        let core = token_core(token);
        if core.len() == 1 {
            if let Some(next_token) = tokens.get(index + 1) {
                let next_core = token_core(next_token);
                if next_core.starts_with(&format!("{core}'")) {
                    continue;
                }
            }
        }

        out.push(token.clone());
    }

    out
}

// ── Token Utilities ──────────────────────────────────────────────────────────

fn token_core(token: &str) -> String {
    token
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
        .to_lowercase()
}

fn token_has_pause(token: &str) -> bool {
    token.ends_with(',')
        || token.ends_with(';')
        || token.ends_with(':')
        || token.ends_with('.')
        || token.ends_with('!')
        || token.ends_with('?')
        || token.ends_with('—')
        || token.ends_with('–')
}

fn is_punctuation_only(token: &str) -> bool {
    !token.chars().any(char::is_alphanumeric)
}

fn normalize_spacing(text: &str) -> String {
    let mut out = text.to_string();
    for (from, to) in [
        (" ,", ","),
        (" .", "."),
        (" !", "!"),
        (" ?", "?"),
        (" ;", ";"),
        (" :", ":"),
    ] {
        out = out.replace(from, to);
    }

    collapse_whitespace(&out)
        .trim_matches(',')
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::format_text;

    #[test]
    fn keeps_existing_basic_formatting() {
        assert_eq!(format_text("i cant do this"), "I can't do this.");
    }

    #[test]
    fn removes_stutter_with_ellipsis() {
        assert_eq!(
            format_text("i... i... i... i'm actually ready"),
            "I'm actually ready."
        );
    }

    #[test]
    fn collapses_repeated_words_and_phrases() {
        assert_eq!(format_text("the the the thing"), "The thing.");
        assert_eq!(format_text("we can we can do this"), "We can do this.");
    }

    #[test]
    fn removes_fillers_but_keeps_like_verb() {
        assert_eq!(
            format_text("um I like it, like, a lot, you know,"),
            "I like it, a lot."
        );
    }

    #[test]
    fn keeps_only_restart_after_false_start() {
        assert_eq!(
            format_text("I want to— actually let me fix that"),
            "Actually let me fix that."
        );
    }
}
