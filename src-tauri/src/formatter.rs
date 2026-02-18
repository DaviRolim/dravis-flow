pub fn format_text(input: &str) -> String {
    let mut text = input.trim().replace(" i ", " I ");
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
