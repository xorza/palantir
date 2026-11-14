//! [`EffectError`]: why [`Ui::load_effect`](crate::Ui::load_effect) refused an effect.

use std::error;
use std::fmt;
use std::ops::Range;

use naga::front::wgsl::ParseError;
use naga::valid::ValidationError;
use naga::{Span, WithSpan};

use crate::effect::stock::RESERVED_PREFIX;
use crate::effect::{MAX_IMAGE_SLOTS, MAX_PARAMS_SIZE};
use crate::primitives::effect::effect_params::ParamType;

/// Why an effect did not load. The effect loaded before under the name, if any, keeps painting.
#[derive(Debug)]
pub enum EffectError {
    /// The WGSL does not parse.
    Parse(WgslError),
    /// The WGSL parses but does not validate.
    Validation(WgslError),
    /// The WGSL declares no function named `effect`.
    MissingEntry,
    /// `effect`'s signature is not the contract's ([`EffectSource`](crate::EffectSource)).
    Entry(EntryMismatch),
    /// The parameter struct the WGSL takes does not match `U`'s fields.
    ParamLayout(ParamMismatch),
    /// `U` is larger than an instance holds.
    ParamsTooLarge {
        /// `U`'s size in bytes.
        size: usize,
    },
    /// The source asks for more image slots than an effect binds.
    TooManyImageSlots {
        /// The slots asked for.
        slots: u8,
    },
    /// The WGSL declares a binding of its own: palantir binds every resource an effect reads, an
    /// image through a slot ([`EffectSource::image_slots`](crate::EffectSource::image_slots)).
    Binding {
        /// The bound variable's name.
        name: String,
    },
    /// The name starts with `palantir.`, the prefix of palantir's stock effects, as
    /// [`EffectName::RIPPLE`](crate::EffectName::RIPPLE).
    ReservedName,
}

/// Why the WGSL naga read does not parse or validate: naga's error over the text it read, the
/// app's WGSL with palantir's prelude around it. Opaque, so no parser type reaches the surface,
/// as an icon's error keeps `usvg`'s inside; its `Display` is naga's message.
pub struct WgslError {
    failure: WgslFailure,
    /// The text naga read.
    text: String,
    /// Where the app's WGSL lies in [`Self::text`].
    app: Range<usize>,
}

/// naga's error, boxed as each is large.
enum WgslFailure {
    Parse(Box<ParseError>),
    Validation(Box<WithSpan<ValidationError>>),
}

impl WgslError {
    pub(crate) fn parse(source: ParseError, text: String, app: Range<usize>) -> Self {
        Self {
            failure: WgslFailure::Parse(Box::new(source)),
            text,
            app,
        }
    }

    pub(crate) fn validation(
        source: WithSpan<ValidationError>,
        text: String,
        app: Range<usize>,
    ) -> Self {
        Self {
            failure: WgslFailure::Validation(Box::new(source)),
            text,
            app,
        }
    }

    /// naga's report over the text it read, its message with the lines it points at, as a
    /// compiler prints one. An error at the end of the app's text, as an unclosed `fn`, points at
    /// the prelude's first line after it.
    pub fn diagnostic(&self) -> String {
        match &self.failure {
            WgslFailure::Parse(error) => error.emit_to_string(&self.text),
            WgslFailure::Validation(error) => error.emit_to_string(&self.text),
        }
    }

    /// The byte range in the app's WGSL of the first place naga points at there, `None` where it
    /// points only into palantir's prelude or nowhere: a name the app redefines points at both.
    pub fn span(&self) -> Option<Range<usize>> {
        let in_app = |span: Span| {
            let range = span.to_range()?;
            (range.start >= self.app.start && range.end <= self.app.end)
                .then(|| range.start - self.app.start..range.end - self.app.start)
        };
        match &self.failure {
            WgslFailure::Parse(error) => error.labels().find_map(|(span, _)| in_app(span)),
            WgslFailure::Validation(error) => error.spans().find_map(|&(span, _)| in_app(span)),
        }
    }
}

/// Summarized: the text is the whole prelude.
impl fmt::Debug for WgslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WgslError")
            .field("message", &format_args!("{self}"))
            .field("span", &self.span())
            .finish_non_exhaustive()
    }
}

/// naga's message alone; [`WgslError::diagnostic`] prints it with its lines.
impl fmt::Display for WgslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.failure {
            WgslFailure::Parse(error) => f.write_str(error.message()),
            WgslFailure::Validation(error) => error.fmt(f),
        }
    }
}

/// Transparent over naga's error, whose message the `Display` already is.
impl error::Error for WgslError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self.failure {
            WgslFailure::Parse(_) => None,
            WgslFailure::Validation(error) => error.source(),
        }
    }
}

/// How `effect`'s signature departs from the contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryMismatch {
    /// It takes another number of arguments than the contract.
    ArgumentCount {
        /// The contract's: 2 with parameters, 1 without.
        want: usize,
        /// The WGSL's.
        got: usize,
    },
    /// It carries a stage attribute, as `@fragment`: palantir owns the stages and calls it from
    /// its own.
    Stage,
    /// Its first argument is not an `EffectInput`.
    Input,
    /// Its second argument is not a struct.
    Params,
    /// It returns no `EffectOutput`.
    Output,
}

/// How the WGSL parameter struct departs from `U`'s fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamMismatch {
    /// The struct has another number of members than `U` has fields.
    FieldCount {
        /// The WGSL struct's members.
        wgsl: usize,
        /// `U`'s fields.
        rust: usize,
    },
    /// A member has another name than `U`'s field in its place.
    Name {
        /// The member's place, from 0.
        idx: usize,
        /// The WGSL member's name.
        wgsl: String,
        /// `U`'s field's name.
        rust: &'static str,
    },
    /// A field sits at another offset in the WGSL struct than in `U`.
    Offset {
        /// The field's name.
        name: &'static str,
        /// Its offset in the WGSL struct, in bytes.
        wgsl: u32,
        /// Its offset in `U`, in bytes.
        rust: usize,
    },
    /// A member has a WGSL type `U`'s field does not stand for.
    Type {
        /// The field's name.
        name: &'static str,
        /// The type `U`'s field stands for.
        rust: ParamType,
    },
    /// The struct spans other bytes than `U`, so an array of them strides apart.
    Size {
        /// The WGSL struct's span, in bytes.
        wgsl: u32,
        /// `U`'s size, in bytes.
        rust: usize,
    },
}

impl fmt::Display for EffectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(_) => f.write_str("the effect's WGSL does not parse"),
            Self::Validation(_) => f.write_str("the effect's WGSL does not validate"),
            Self::MissingEntry => f.write_str("the effect's WGSL declares no `fn effect`"),
            Self::Entry(mismatch) => write!(f, "`fn effect` {mismatch}"),
            Self::ParamLayout(mismatch) => write!(f, "the parameter struct {mismatch}"),
            Self::ParamsTooLarge { size } => write!(
                f,
                "the parameters are {size} B, past the {MAX_PARAMS_SIZE} B an instance holds"
            ),
            Self::TooManyImageSlots { slots } => write!(
                f,
                "the effect asks for {slots} image slots, past the {MAX_IMAGE_SLOTS} an effect binds"
            ),
            Self::Binding { name } => write!(
                f,
                "the effect's WGSL binds `{name}` itself, where palantir binds every resource"
            ),
            Self::ReservedName => write!(
                f,
                "the effect's name starts with `{RESERVED_PREFIX}`, which palantir's stock effects hold"
            ),
        }
    }
}

impl fmt::Display for EntryMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ArgumentCount { want, got } => {
                write!(f, "takes {got} arguments where the contract takes {want}")
            }
            Self::Stage => f.write_str("carries a stage attribute, which palantir's stage owns"),
            Self::Input => f.write_str("takes no `EffectInput` first"),
            Self::Params => f.write_str("takes no struct of parameters second"),
            Self::Output => f.write_str("returns no `EffectOutput`"),
        }
    }
}

impl fmt::Display for ParamMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldCount { wgsl, rust } => {
                write!(
                    f,
                    "has {wgsl} members where the Rust struct has {rust} fields"
                )
            }
            Self::Name { idx, wgsl, rust } => {
                write!(
                    f,
                    "names member {idx} `{wgsl}` where the Rust struct names it `{rust}`"
                )
            }
            Self::Offset { name, wgsl, rust } => write!(
                f,
                "puts `{name}` at byte {wgsl} where the Rust struct puts it at {rust}"
            ),
            Self::Type { name, rust } => write!(
                f,
                "types `{name}` other than the Rust field's `{}`",
                rust.wgsl()
            ),
            Self::Size { wgsl, rust } => {
                write!(f, "spans {wgsl} B where the Rust struct spans {rust} B")
            }
        }
    }
}

impl error::Error for EffectError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Parse(source) | Self::Validation(source) => Some(source),
            _ => None,
        }
    }
}
