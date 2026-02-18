pub fn format_text(input: &str) -> String {
    let cleaned = smart_cleanup(input);
    let mut text = cleaned.trim().replace(" i ", " I ");
    if text.is_empty() {
        return text;
    }

    if text == "i" {
        text = "I".to_string();
    }

    let contractions = [
        ("dont", "don't"),
        ("cant", "can't"),
        ("wont", "won't"),
        ("im", "I'm"),
        ("ive", "I've"),
        ("id", "I'd"),
        ("ill", "I'll"),
    ];

    let mut lowered = text.to_lowercase();
    for (a, b) in contractions {
        lowered = lowered.replace(a, b);
    }

    let mut chars = lowered.chars();
    if let Some(first) = chars.next() {
        text = first.to_uppercase().collect::<String>() + chars.as_str();
    } else {
        text = lowered;
    }

    if !text.ends_with(['.', '!', '?']) {
        text.push('.');
    }

    text
}

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

    collapse_whitespace(&out).trim_matches(',').trim().to_string()
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
        assert_eq!(format_text("i... i... i... i'm actually ready"), "I'm actually ready.");
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
