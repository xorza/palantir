//! [`Script`]: a shot script parsed line by line, its header and its steps.

use std::error;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

use palantir::glam::{UVec2, Vec2};
use palantir::{Action, Locator, PointerButton, Rect, Shortcut, ShortcutMods};

/// What a script sets before its first step; each has the default [`Header::default`] names.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Header {
    /// The window, in physical pixels.
    pub(crate) size: UVec2,
    /// Physical pixels per logical pixel.
    pub(crate) scale: f32,
    /// The page the shell opens on, by its label; the first page for none.
    pub(crate) page: Option<String>,
    /// The longest side a picture is written at.
    pub(crate) max_side: u32,
}

impl Default for Header {
    /// 1600 × 1000 at scale 1, the first page, pictures up to 2576 px a side.
    fn default() -> Self {
        Self {
            size: UVec2::new(1600, 1000),
            scale: 1.0,
            page: None,
            max_side: 2576,
        }
    }
}

/// One step of a script, with the line it is on.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Step {
    /// From 1.
    pub(crate) line: usize,
    pub(crate) kind: StepKind,
}

/// What a step does.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StepKind {
    /// Play an action, as a person at the window would.
    Act(Action),
    /// Write a picture of the window or a region of it.
    Shot {
        name: String,
        region: Option<Region>,
        /// The magnification of the region, nearest; 1 fits the picture to the largest side.
        zoom: u32,
    },
    /// Print the sRGB bytes under one logical pixel.
    Probe(Vec2),
    /// List the visible widgets whose text holds the filter.
    Widgets(Option<String>),
}

/// The part of the window a shot takes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Region {
    /// What a locator finds.
    At(Locator),
    /// A logical rect.
    Rect(Rect),
}

/// How a press is made: the button, and the modifiers held through it.
#[derive(Clone, Copy, Debug)]
struct Press {
    button: PointerButton,
    mods: ShortcutMods,
}

/// A parsed script.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Script {
    pub(crate) header: Header,
    pub(crate) steps: Vec<Step>,
}

/// Why a line does not parse, with its line from 1.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParseError {
    pub(crate) line: usize,
    pub(crate) kind: ParseErrorKind,
}

/// The cases a line can fail by.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ParseErrorKind {
    /// A first word that names no instruction.
    Unknown(String),
    /// A header line after the first step.
    LateHeader(String),
    /// A word missing at the end of the line: what it expected.
    Missing(&'static str),
    /// A word that does not read as what the place expects.
    Bad {
        expected: &'static str,
        found: String,
    },
    /// A word left over after the line was read.
    Extra(String),
    /// A quote that does not end.
    OpenQuote,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: ", self.line)?;
        match &self.kind {
            ParseErrorKind::Unknown(word) => write!(
                f,
                "unknown instruction `{word}`; expected size, scale, page, max-side, click, \
                 double-click, hover, drag, scroll, pinch, key, type, wait, shot, probe or widgets"
            ),
            ParseErrorKind::LateHeader(word) => {
                write!(f, "`{word}` sets the header, before the first step")
            }
            ParseErrorKind::Missing(expected) => write!(f, "expected {expected} at the end"),
            ParseErrorKind::Bad { expected, found } => {
                write!(f, "expected {expected}, found `{found}`")
            }
            ParseErrorKind::Extra(word) => write!(f, "unexpected `{word}` at the end"),
            ParseErrorKind::OpenQuote => f.write_str("a quote does not end"),
        }
    }
}

impl error::Error for ParseError {}

impl Script {
    /// Read `text`, one instruction a line; a blank line and one that starts with `//` are
    /// skipped.
    pub(crate) fn parse(text: &str) -> Result<Self, ParseError> {
        let mut header = Header::default();
        let mut steps = Vec::new();
        for (idx, raw) in text.lines().enumerate() {
            let line = idx + 1;
            let fail = |kind| ParseError { line, kind };
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            let mut words = Words::of(trimmed).map_err(fail)?;
            let head = words.next_word("an instruction").map_err(fail)?;
            let header_word = matches!(head.as_str(), "size" | "scale" | "page" | "max-side");
            if header_word && !steps.is_empty() {
                return Err(fail(ParseErrorKind::LateHeader(head)));
            }
            let kind = match head.as_str() {
                "size" => {
                    header.size = words.size().map_err(fail)?;
                    None
                }
                "scale" => {
                    header.scale = words.positive("a scale above 0").map_err(fail)?;
                    None
                }
                "page" => {
                    header.page = Some(words.text("a quoted page label").map_err(fail)?);
                    None
                }
                "max-side" => {
                    header.max_side = words.count("a largest side in pixels").map_err(fail)?;
                    None
                }
                _ => Some(step(&head, &mut words).map_err(fail)?),
            };
            words.end().map_err(fail)?;
            if let Some(kind) = kind {
                steps.push(Step { line, kind });
            }
        }
        Ok(Self { header, steps })
    }
}

/// The step `head` names, its words read from `words`.
fn step(head: &str, words: &mut Words) -> Result<StepKind, ParseErrorKind> {
    let act = |action| Ok(StepKind::Act(action));
    match head {
        "click" => {
            let at = words.locator()?;
            let Press { button, mods } = words.press()?;
            act(Action::Click { at, button, mods })
        }
        "double-click" => act(Action::DoubleClick(words.locator()?)),
        "hover" => act(Action::Move(words.locator()?)),
        "drag" => {
            let from = words.locator()?;
            words.keyword("to")?;
            let to = words.locator()?;
            let Press { button, mods } = words.press()?;
            act(Action::Drag {
                from,
                to,
                button,
                mods,
                steps: Action::DRAG_STEPS,
            })
        }
        "scroll" => {
            let at = words.locator()?;
            let delta = words.pair("a scroll dx,dy")?;
            act(Action::Scroll { at, delta })
        }
        "pinch" => {
            let at = words.locator()?;
            let factor = words.positive("a pinch factor above 0")?;
            act(Action::Pinch { at, factor })
        }
        "key" => {
            let word = words.next_word("a key, as ctrl+z")?;
            let shortcut = word.parse::<Shortcut>().map_err(|_| ParseErrorKind::Bad {
                expected: "a key, as ctrl+z or enter",
                found: word,
            })?;
            act(Action::Key(shortcut))
        }
        "type" => act(Action::Type(words.text("quoted text")?)),
        "wait" => act(Action::Wait(words.duration()?)),
        "shot" => {
            let name = words.next_word("a shot name")?;
            let mut region = None;
            let mut zoom = 1;
            while let Some(option) = words.peek() {
                match option {
                    "region" => {
                        words.next_word("region")?;
                        region = Some(words.region()?);
                    }
                    "zoom" => {
                        words.next_word("zoom")?;
                        zoom = words.count("a zoom of 1 or more")?;
                    }
                    _ => break,
                }
            }
            Ok(StepKind::Shot { name, region, zoom })
        }
        "probe" => Ok(StepKind::Probe(words.pair("a logical point x,y")?)),
        "widgets" => Ok(StepKind::Widgets(
            words.peek().is_some().then(|| words.word_or_text()),
        )),
        _ => Err(ParseErrorKind::Unknown(head.to_owned())),
    }
}

/// The words of one line: a quoted string with what follows it up to a space is one word.
#[derive(Debug)]
struct Words {
    words: Vec<String>,
    next: usize,
}

impl Words {
    fn of(line: &str) -> Result<Self, ParseErrorKind> {
        let mut words = Vec::new();
        let mut chars = line.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
                continue;
            }
            let mut word = String::new();
            let mut quoted = false;
            while let Some(&c) = chars.peek() {
                if c == '"' {
                    quoted = !quoted;
                } else if c == '\\' && quoted {
                    word.push(c);
                    chars.next();
                    let escaped = chars.next().ok_or(ParseErrorKind::OpenQuote)?;
                    word.push(escaped);
                    continue;
                } else if c.is_whitespace() && !quoted {
                    break;
                }
                word.push(c);
                chars.next();
            }
            if quoted {
                return Err(ParseErrorKind::OpenQuote);
            }
            words.push(word);
        }
        Ok(Self { words, next: 0 })
    }

    fn peek(&self) -> Option<&str> {
        self.words.get(self.next).map(String::as_str)
    }

    fn next_word(&mut self, expected: &'static str) -> Result<String, ParseErrorKind> {
        let word = self
            .words
            .get(self.next)
            .cloned()
            .ok_or(ParseErrorKind::Missing(expected))?;
        self.next += 1;
        Ok(word)
    }

    fn end(&self) -> Result<(), ParseErrorKind> {
        self.peek()
            .map_or(Ok(()), |word| Err(ParseErrorKind::Extra(word.to_owned())))
    }

    fn keyword(&mut self, keyword: &'static str) -> Result<(), ParseErrorKind> {
        let word = self.next_word(keyword)?;
        if word == keyword {
            Ok(())
        } else {
            Err(ParseErrorKind::Bad {
                expected: keyword,
                found: word,
            })
        }
    }

    /// A quoted string, its quotes and escapes taken off.
    fn text(&mut self, expected: &'static str) -> Result<String, ParseErrorKind> {
        let word = self.next_word(expected)?;
        unquote(&word)
            .filter(|(_, rest)| rest.is_empty())
            .map(|(text, _)| text)
            .ok_or(ParseErrorKind::Bad {
                expected,
                found: word,
            })
    }

    /// A quoted string, or a bare word as it is.
    fn word_or_text(&mut self) -> String {
        let word = self.words[self.next].clone();
        self.next += 1;
        let text = unquote(&word)
            .filter(|(_, rest)| rest.is_empty())
            .map(|(text, _)| text);
        text.unwrap_or(word)
    }

    fn positive(&mut self, expected: &'static str) -> Result<f32, ParseErrorKind> {
        let word = self.next_word(expected)?;
        number(&word)
            .filter(|value| *value > 0.0)
            .ok_or(ParseErrorKind::Bad {
                expected,
                found: word,
            })
    }

    fn count(&mut self, expected: &'static str) -> Result<u32, ParseErrorKind> {
        let word = self.next_word(expected)?;
        word.parse::<u32>()
            .ok()
            .filter(|count| *count > 0)
            .ok_or(ParseErrorKind::Bad {
                expected,
                found: word,
            })
    }

    fn pair(&mut self, expected: &'static str) -> Result<Vec2, ParseErrorKind> {
        let word = self.next_word(expected)?;
        pair(&word).ok_or(ParseErrorKind::Bad {
            expected,
            found: word,
        })
    }

    fn size(&mut self) -> Result<UVec2, ParseErrorKind> {
        let expected = "a size WxH in physical pixels";
        let word = self.next_word(expected)?;
        word.split_once('x')
            .and_then(|(w, h)| Some(UVec2::new(w.parse().ok()?, h.parse().ok()?)))
            .filter(|size| size.x > 0 && size.y > 0)
            .ok_or(ParseErrorKind::Bad {
                expected,
                found: word,
            })
    }

    fn duration(&mut self) -> Result<Duration, ParseErrorKind> {
        let expected = "a time as 300ms or 2s";
        let word = self.next_word(expected)?;
        let duration = if let Some(ms) = word.strip_suffix("ms") {
            ms.parse::<u64>().ok().map(Duration::from_millis)
        } else {
            word.strip_suffix('s')
                .and_then(|seconds| seconds.parse::<f64>().ok())
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .map(Duration::from_secs_f64)
        };
        duration.ok_or(ParseErrorKind::Bad {
            expected,
            found: word,
        })
    }

    /// `"text"`, `"text"#2`, `x,y`, either with `+dx,dy`.
    fn locator(&mut self) -> Result<Locator, ParseErrorKind> {
        let expected = "a place: \"text\", \"text\"#2 or x,y, with +dx,dy to move it";
        let word = self.next_word(expected)?;
        locator(&word).ok_or(ParseErrorKind::Bad {
            expected,
            found: word,
        })
    }

    /// `x,y,w,h`, or a locator whose rect the region is.
    fn region(&mut self) -> Result<Region, ParseErrorKind> {
        let word = self.next_word("a region")?;
        let parts: Option<Vec<f32>> = word.split(',').map(number).collect();
        if let Some([x, y, w, h]) = parts.as_deref()
            && *w > 0.0
            && *h > 0.0
        {
            return Ok(Region::Rect(Rect::new(*x, *y, *w, *h)));
        }
        locator(&word).map(Region::At).ok_or(ParseErrorKind::Bad {
            expected: "a region: x,y,w,h or a place",
            found: word,
        })
    }

    /// `button left|right|middle` and `mods ctrl+shift`, in any order, each at most once.
    fn press(&mut self) -> Result<Press, ParseErrorKind> {
        let mut button = PointerButton::Left;
        let mut mods = ShortcutMods::NONE;
        while let Some(option) = self.peek() {
            match option {
                "button" => {
                    self.next += 1;
                    let word = self.next_word("left, right or middle")?;
                    button = match word.as_str() {
                        "left" => PointerButton::Left,
                        "right" => PointerButton::Right,
                        "middle" => PointerButton::Middle,
                        _ => {
                            return Err(ParseErrorKind::Bad {
                                expected: "left, right or middle",
                                found: word,
                            });
                        }
                    };
                }
                "mods" => {
                    self.next += 1;
                    let word = self.next_word("modifiers, as ctrl+shift")?;
                    // A shortcut with a key, so its parse reads the modifiers alone.
                    mods = format!("{word}+a")
                        .parse::<Shortcut>()
                        .map_err(|_| ParseErrorKind::Bad {
                            expected: "modifiers, as ctrl+shift",
                            found: word,
                        })?
                        .mods;
                }
                _ => break,
            }
        }
        Ok(Press { button, mods })
    }
}

/// The text between a word's leading quotes, its escapes taken off, and what follows the closing
/// quote; `None` for a word that does not start with a quote.
fn unquote(word: &str) -> Option<(String, &str)> {
    let rest = word.strip_prefix('"')?;
    let mut text = String::new();
    let mut chars = rest.char_indices();
    while let Some((idx, c)) = chars.next() {
        match c {
            '"' => return Some((text, &rest[idx + 1..])),
            '\\' => text.push(chars.next()?.1),
            c => text.push(c),
        }
    }
    None
}

fn number(word: &str) -> Option<f32> {
    word.parse::<f32>().ok().filter(|value| value.is_finite())
}

fn pair(word: &str) -> Option<Vec2> {
    let (x, y) = word.split_once(',')?;
    Some(Vec2::new(number(x)?, number(y)?))
}

fn locator(word: &str) -> Option<Locator> {
    let (base, offset) = if let Some((text, rest)) = unquote(word) {
        let text = Locator::Text(text);
        if let Some(rest) = rest.strip_prefix('#') {
            let end = rest.find('+').unwrap_or(rest.len());
            let nth = rest[..end].parse::<usize>().ok()?;
            (Locator::Nth(Box::new(text), nth), &rest[end..])
        } else {
            (text, rest)
        }
    } else {
        // `x,y` with an optional `+dx,dy`: the offset starts at the `+` after the comma.
        let comma = word.find(',')?;
        let end = word[comma..].find('+').map_or(word.len(), |at| comma + at);
        (Locator::Point(pair(&word[..end])?), &word[end..])
    };
    if offset.is_empty() {
        return Some(base);
    }
    let by = pair(offset.strip_prefix('+')?)?;
    Some(Locator::Offset(Box::new(base), by))
}
