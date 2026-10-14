//! Performance scripts: dialogue with inline `[cue]` events Drama 3 performs
//! rather than speaks.

const MAX_CUE_LENGTH: usize = 40;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cue<'a> {
    Speech(&'a str),
    Event(&'a str),
}

/// Splits a script into spoken text and event cues. Brackets that do not hold
/// a short word-like cue stay part of the spoken text.
#[must_use]
pub fn cues(script: &str) -> Vec<Cue<'_>> {
    let mut cues = Vec::new();
    let mut speech_start = 0;
    let mut cursor = 0;
    while let Some(open) = script[cursor..].find('[').map(|offset| cursor + offset) {
        let Some(close) = script[open + 1..].find(']').map(|offset| open + 1 + offset) else {
            break;
        };
        let inner = &script[open + 1..close];
        if inner.contains('[') {
            cursor = open + 1 + inner.rfind('[').expect("inner contains '['");
            continue;
        }
        let event = inner.trim();
        if is_event(event) {
            push_speech(&mut cues, &script[speech_start..open]);
            cues.push(Cue::Event(event));
            speech_start = close + 1;
        }
        cursor = close + 1;
    }
    push_speech(&mut cues, &script[speech_start..]);
    cues
}

/// The words an actor speaks, with cues removed and whitespace collapsed.
#[must_use]
pub fn spoken(script: &str) -> String {
    let mut words = String::new();
    for cue in cues(script) {
        if let Cue::Speech(text) = cue {
            for word in text.split_whitespace() {
                if !words.is_empty() {
                    words.push(' ');
                }
                words.push_str(word);
            }
        }
    }
    words
}

/// Removes Drama 3 control tokens (`<|speaker:0|>` and similar) so scene text
/// cannot steer speaker routing, and collapses whitespace.
#[must_use]
pub fn sanitize(script: &str) -> String {
    let mut clean = String::with_capacity(script.len());
    let mut rest = script;
    while let Some(start) = rest.find("<|") {
        clean.push_str(&rest[..start]);
        match rest[start + 2..].find("|>") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => rest = "",
        }
    }
    clean.push_str(rest);
    clean.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_event(cue: &str) -> bool {
    !cue.is_empty()
        && cue.len() <= MAX_CUE_LENGTH
        && cue.chars().any(char::is_alphabetic)
        && cue
            .chars()
            .all(|character| character.is_alphabetic() || matches!(character, ' ' | '-' | '\''))
}

fn push_speech<'a>(cues: &mut Vec<Cue<'a>>, text: &'a str) {
    if !text.trim().is_empty() {
        cues.push(Cue::Speech(text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_events_from_speech() {
        assert_eq!(
            cues("[sighs] Fine. If we miss it, it's on you. [laughs] Well, maybe that's fair."),
            [
                Cue::Event("sighs"),
                Cue::Speech(" Fine. If we miss it, it's on you. "),
                Cue::Event("laughs"),
                Cue::Speech(" Well, maybe that's fair."),
            ]
        );
    }

    #[test]
    fn keeps_non_cue_brackets_as_speech() {
        assert_eq!(spoken("Room [42] is [ ] here"), "Room [42] is [ ] here");
        assert_eq!(spoken("[[sighs] ok"), "[ ok");
        assert_eq!(spoken("unclosed [sighs"), "unclosed [sighs");
    }

    #[test]
    fn spoken_text_ignores_cues_and_spacing() {
        assert_eq!(
            spoken("[sighs]Fine.   [laughs] Well,\nmaybe."),
            "Fine. Well, maybe."
        );
        assert_eq!(spoken("[whispering] [gasps]"), "");
    }

    #[test]
    fn sanitize_removes_control_tokens() {
        assert_eq!(
            sanitize("<|speaker:3|> [sighs]  Fine.<|im_end|> ok <|broken"),
            "[sighs] Fine. ok"
        );
    }
}
