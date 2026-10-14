//! Post-translation LLM step that inserts cues and names speakers; spoken
//! words must match the translation so text and audio agree.

use anyhow::{Context, Result};
use indoc::formatdoc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::script;

const MAX_CHARACTER_CHARS: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectionLine {
    pub source: String,
    pub translation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectionRequest {
    pub lines: Vec<DirectionLine>,
    /// Display name of the translation language, such as "English".
    pub language: String,
    pub instructions: Option<String>,
}

/// Provider-neutral structured prompt; the caller owns the LLM transport.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectionPrompt {
    /// Schema name for providers that require one.
    pub name: &'static str,
    pub system: String,
    pub user: String,
    pub schema: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Direction {
    pub script: String,
    pub character: Option<String>,
}

pub fn prompt(request: &DirectionRequest) -> Result<DirectionPrompt> {
    let language = &request.language;
    let mut system = formatdoc! {"
        You are a voice director preparing translated manga dialogue for an expressive text-to-speech actor.

        Direction requirements:
        - For every line, return `script`: the line's translation word for word, with optional performance cues inserted in square brackets, such as [sighs], [laughs], [whispers], [gasps], [shouting], [sobbing], [nervous], or [pause].
        - Write cues as short lowercase English words even though the dialogue is in {language}. Put a cue directly before the words it colors, or use it alone for a pure reaction.
        - Never change, add, remove, reorder, or translate the dialogue words or punctuation; only insert cues.
        - Add a cue only where the source text, the wording, or the image clearly implies a sound or a delivery. Most lines need zero or one cue.
        - Return `character`: a short name for whoever speaks the line. Reuse exactly the same name for the same speaker across lines. Use \"narrator\" for captions and an empty string when the speaker is unknown.

        Output requirements:
        - Each input line has a numeric `id`.
        - Return only a JSON object whose `lines` array contains one object with `id`, `character`, and `script` for every input line.
        - Copy every input ID exactly once; never merge, split, omit, duplicate, or add lines."
    };
    if let Some(instructions) = request
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|instructions| !instructions.is_empty())
    {
        system.push_str("\n\nAdditional instructions:\n");
        system.push_str(instructions);
    }
    let user = serde_json::to_string(&DirectionInput {
        language,
        lines: request
            .lines
            .iter()
            .enumerate()
            .map(|(id, line)| InputLine {
                id,
                source: &line.source,
                translation: &line.translation,
            })
            .collect(),
    })
    .context("failed to encode direction input")?;
    Ok(DirectionPrompt {
        name: "manga_dialogue",
        system,
        user,
        schema: output_schema(request.lines.len()),
    })
}

/// Lines the model omitted, duplicated, or reworded keep their translation
/// as an uncued script.
pub fn parse(text: &str, lines: &[DirectionLine]) -> Result<Vec<Direction>> {
    let output: DirectionOutput =
        serde_json::from_str(text).context("direction response is not the expected JSON")?;
    let mut directions: Vec<Option<Direction>> = vec![None; lines.len()];
    for line in output.lines {
        let Some(slot) = directions.get_mut(line.id) else {
            continue;
        };
        if slot.is_some() {
            continue;
        }
        let translation = &lines[line.id].translation;
        let cleaned = script::sanitize(&line.script);
        let script = if script::spoken(&cleaned) == script::spoken(translation) {
            cleaned
        } else {
            tracing::warn!(
                id = line.id,
                "direction changed the dialogue words; keeping the translation"
            );
            translation.clone()
        };
        *slot = Some(Direction {
            script,
            character: character(&line.character),
        });
    }
    Ok(directions
        .into_iter()
        .zip(lines)
        .map(|(direction, line)| {
            direction.unwrap_or_else(|| Direction {
                script: line.translation.clone(),
                character: None,
            })
        })
        .collect())
}

fn character(name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = name.chars().take(MAX_CHARACTER_CHARS).collect();
    (!name.is_empty()).then_some(name)
}

fn output_schema(expected: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "minItems": expected,
                "maxItems": expected,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "integer",
                            "minimum": 0,
                            "maximum": expected.saturating_sub(1),
                            "description": "The ID copied from the corresponding input line."
                        },
                        "character": {
                            "type": "string",
                            "description": "Who speaks the line; empty when unknown."
                        },
                        "script": {
                            "type": "string",
                            "description": "The translation with bracketed performance cues inserted."
                        }
                    },
                    "required": ["id", "character", "script"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["lines"],
        "additionalProperties": false
    })
}

#[derive(Serialize)]
struct DirectionInput<'a> {
    language: &'a str,
    lines: Vec<InputLine<'a>>,
}

#[derive(Serialize)]
struct InputLine<'a> {
    id: usize,
    source: &'a str,
    translation: &'a str,
}

#[derive(Deserialize)]
struct DirectionOutput {
    lines: Vec<OutputLine>,
}

#[derive(Deserialize)]
struct OutputLine {
    id: usize,
    character: String,
    script: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Vec<DirectionLine> {
        vec![
            DirectionLine {
                source: "はぁ…いいよ。".to_owned(),
                translation: "Fine. If we miss it, it's on you.".to_owned(),
            },
            DirectionLine {
                source: "ははっ".to_owned(),
                translation: "Well, maybe that's fair.".to_owned(),
            },
        ]
    }

    #[test]
    fn prompt_lists_every_line_with_its_id() {
        let prompt = prompt(&DirectionRequest {
            lines: lines(),
            language: "English".to_owned(),
            instructions: Some("Keep it subtle.".to_owned()),
        })
        .unwrap();
        assert!(prompt.system.contains("dialogue is in English"));
        assert!(
            prompt
                .system
                .ends_with("Additional instructions:\nKeep it subtle.")
        );
        let input: Value = serde_json::from_str(&prompt.user).unwrap();
        assert_eq!(input["lines"][1]["id"], 1);
        assert_eq!(input["lines"][1]["translation"], "Well, maybe that's fair.");
        assert_eq!(prompt.schema["properties"]["lines"]["maxItems"], 2);
    }

    #[test]
    fn parse_keeps_cued_scripts_with_identical_words() {
        let output = r#"{"lines":[
            {"id":1,"character":"Mika","script":"[laughs] Well, maybe that's fair."},
            {"id":0,"character":"  Mika ","script":"[sighs] Fine. If we miss it, it's on you."}
        ]}"#;
        assert_eq!(
            parse(output, &lines()).unwrap(),
            [
                Direction {
                    script: "[sighs] Fine. If we miss it, it's on you.".to_owned(),
                    character: Some("Mika".to_owned()),
                },
                Direction {
                    script: "[laughs] Well, maybe that's fair.".to_owned(),
                    character: Some("Mika".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn parse_rejects_rewritten_words_and_fills_missing_lines() {
        let output = r#"{"lines":[
            {"id":0,"character":"","script":"[sighs] Fine, whatever."},
            {"id":0,"character":"Ren","script":"duplicate"},
            {"id":7,"character":"Ren","script":"out of range"}
        ]}"#;
        assert_eq!(
            parse(output, &lines()).unwrap(),
            [
                Direction {
                    script: "Fine. If we miss it, it's on you.".to_owned(),
                    character: None,
                },
                Direction {
                    script: "Well, maybe that's fair.".to_owned(),
                    character: None,
                },
            ]
        );
    }

    #[test]
    fn parse_strips_control_tokens_from_scripts() {
        let output = r#"{"lines":[
            {"id":0,"character":"Mika","script":"<|speaker:9|>Fine. If we miss it, it's on you."},
            {"id":1,"character":"Mika","script":"Well, maybe that's fair."}
        ]}"#;
        assert_eq!(
            parse(output, &lines()).unwrap()[0].script,
            "Fine. If we miss it, it's on you."
        );
    }
}
