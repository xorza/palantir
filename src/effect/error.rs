//! [`EffectError`]: why [`Ui::load_effect`](crate::Ui::load_effect) refused an effect.

use std::error;
use std::fmt;

use naga::WithSpan;
use naga::front::wgsl::ParseError;
use naga::valid::ValidationError;

use crate::effect::effect_params::ParamType;
use crate::effect::{MAX_IMAGE_SLOTS, MAX_PARAMS_SIZE};

/// Why an effect did not load. The effect loaded before under the name, if any, keeps painting.
#[derive(Debug)]
pub enum EffectError {
    /// The WGSL does not parse.
    Parse {
        /// naga's error, boxed as it is large: `source.emit_to_string(&text)` prints it with its
        /// position.
        source: Box<ParseError>,
        /// The text naga parsed: the app's WGSL, then palantir's prelude. A position in the app's
        /// text reads the same in it; an error at the end of the app's text, as an unclosed
        /// `fn`, points at the prelude's first line.
        text: String,
    },
    /// The WGSL parses but does not validate.
    Validation {
        /// naga's error, boxed as it is large: `source.emit_to_string(&text)` prints it with its
        /// position.
        source: Box<WithSpan<ValidationError>>,
        /// The text naga parsed, as [`Self::Parse`] carries it.
        text: String,
    },
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
            Self::Parse { .. } => f.write_str("the effect's WGSL does not parse"),
            Self::Validation { .. } => f.write_str("the effect's WGSL does not validate"),
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
            Self::Parse { source, .. } => Some(source),
            Self::Validation { source, .. } => Some(source),
            _ => None,
        }
    }
}
