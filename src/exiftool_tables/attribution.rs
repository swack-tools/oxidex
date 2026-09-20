//! Process-wide generated-route attribution switch used only by `genshare`.

use std::sync::OnceLock;

const ENV: &str = "OXIDEX_GENSHARE_SILENCE";

/// A maintained generated-output boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Token {
    Engine,
    LegacyL1,
    LegacyL2,
    Producers,
    Serial,
    Keyed,
}

impl Token {
    const fn bit(self) -> u8 {
        match self {
            Self::Engine => 1,
            Self::LegacyL1 => 2,
            Self::LegacyL2 => 4,
            Self::Producers => 8,
            Self::Serial => 16,
            Self::Keyed => 32,
        }
    }
}

fn parse(spec: &str) -> Result<u8, String> {
    let mut bits = 0;
    for word in spec.split(',') {
        if word.is_empty() || word.trim() != word {
            return Err(format!(
                "invalid OXIDEX_GENSHARE_SILENCE component {word:?}"
            ));
        }
        let bit = match word {
            "engine" => Token::Engine.bit(),
            "legacy-l1" => Token::LegacyL1.bit(),
            "legacy-l2" => Token::LegacyL2.bit(),
            "producers" => Token::Producers.bit(),
            "serial" => Token::Serial.bit(),
            "keyed" => Token::Keyed.bit(),
            "conv" => return Err("OXIDEX_GENSHARE_SILENCE=conv is unsafe".into()),
            other => return Err(format!("unknown OXIDEX_GENSHARE_SILENCE token {other:?}")),
        };
        if bits & bit != 0 {
            return Err(format!("duplicate OXIDEX_GENSHARE_SILENCE token {word:?}"));
        }
        bits |= bit;
    }
    Ok(bits)
}

static SELECTED: OnceLock<Result<u8, String>> = OnceLock::new();

/// Validate the process environment once. The census invokes this policy before
/// it traverses a corpus, so a bad token is never measured as all-MISSING.
pub fn validate() -> Result<(), &'static str> {
    SELECTED
        .get_or_init(|| match std::env::var(ENV) {
            Ok(spec) if !spec.is_empty() => parse(&spec),
            _ => Ok(0),
        })
        .as_ref()
        .map(|_| ())
        .map_err(|error| error.as_str())
}

/// Whether the selected token suppresses only an already-computed outward row.
#[must_use]
pub fn silenced(token: Token) -> bool {
    matches!(SELECTED.get_or_init(|| match std::env::var(ENV) { Ok(spec) if !spec.is_empty() => parse(&spec), _ => Ok(0) }), Ok(bits) if bits & token.bit() != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_the_maintained_tokens() {
        assert_eq!(parse("engine,serial,keyed"), Ok(1 | 16 | 32));
    }
    #[test]
    fn refuses_unsafe_or_unknown_tokens() {
        assert!(parse("conv").is_err());
        assert!(parse("made-up").is_err());
    }

    #[test]
    fn refuses_ambiguous_components() {
        for spec in [
            "engine,engine",
            "engine,,serial",
            ",engine",
            "engine,",
            "engine, serial",
            " engine",
            "",
        ] {
            assert!(parse(spec).is_err(), "{spec:?} must be rejected");
        }
    }
}
