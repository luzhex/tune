//! Module for working with pitches and frequencies.

use crate::note;
use crate::note::Note;
use crate::parse;
use crate::{
    key::PianoKey,
    ratio::Ratio,
    tuning::{Approximation, Tuning},
};
use note::PitchedNote;
use std::ops::{Div, Mul};
use std::str::FromStr;

/// Struct representing the frequency of a pitch.
///
///
/// You can retrieve the absolute frequency of a [`Pitch`] in Hz via [`Pitch::as_hz`].
/// Alternatively, [`Pitch`]es can interact with [`Ratio`]s using [`Ratio::between_pitches`] or the [`Mul`]/[`Div`] operators.
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub struct Pitch {
    hz: f64,
}

impl Pitch {
    /// A more intuitive replacement for [`Pitched::pitch`].
    ///
    /// # Examples
    /// ```
    /// # use assert_approx_eq::assert_approx_eq;
    /// # use tune::note::NoteLetter;
    /// # use tune::pitch::Pitch;
    /// use tune::pitch::Pitched;
    ///
    /// let note = NoteLetter::C.in_octave(4);
    /// assert_approx_eq!(Pitch::of(note).as_hz(), note.pitch().as_hz());
    /// ```
    pub fn of(pitched: impl Pitched) -> Pitch {
        pitched.pitch()
    }

    pub fn from_hz(hz: f64) -> Pitch {
        Pitch { hz }
    }

    pub fn as_hz(self) -> f64 {
        self.hz
    }
}

impl FromStr for Pitch {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.ends_with("Hz") || s.ends_with("hz") {
            let freq = &s[..s.len() - 2];
            let freq = freq
                .parse::<Ratio>()
                .map_err(|e| format!("Invalid frequency: '{}': {}", freq, e))?;
            Ok(Pitch::from_hz(freq.as_float()))
        } else {
            Err("Must end with Hz or hz".to_string())
        }
    }
}

/// Lower a [`Pitch`] by a given [`Ratio`].
///
/// # Examples
///
/// ```
/// # use assert_approx_eq::assert_approx_eq;
/// # use tune::pitch::Pitch;
/// # use tune::ratio::Ratio;
/// assert_approx_eq!((Pitch::from_hz(330.0) / Ratio::from_float(1.5)).as_hz(), 220.0);
/// ```
impl Div<Ratio> for Pitch {
    type Output = Pitch;

    fn div(self, rhs: Ratio) -> Self::Output {
        Pitch::from_hz(self.as_hz() / rhs.as_float())
    }
}

/// Raise a [`Pitch`] by a given [`Ratio`].
///
/// # Examples
///
/// ```
/// # use assert_approx_eq::assert_approx_eq;
/// # use tune::pitch::Pitch;
/// # use tune::ratio::Ratio;
/// assert_approx_eq!((Pitch::from_hz(220.0) * Ratio::from_float(1.5)).as_hz(), 330.0);
/// ```
impl Mul<Ratio> for Pitch {
    type Output = Pitch;

    fn mul(self, rhs: Ratio) -> Self::Output {
        Pitch::from_hz(self.as_hz() * rhs.as_float())
    }
}

/// Objects which have a [`Pitch`] assigned.
pub trait Pitched: Copy {
    /// Retrieves the [`Pitch`] of the [`Pitched`] object.
    ///
    /// # Examples
    ///
    /// ```
    /// # use assert_approx_eq::assert_approx_eq;
    /// # use tune::note::NoteLetter;
    /// # use tune::pitch::Pitch;
    /// use tune::pitch::Pitched;
    ///
    /// assert_approx_eq!(Pitch::from_hz(123.456).pitch().as_hz(), 123.456);
    /// assert_approx_eq!(NoteLetter::A.in_octave(5).pitch().as_hz(), 880.0);
    /// ```
    fn pitch(self) -> Pitch;

    /// Finds a key or note for any [`Pitched`] object in the given `tuning`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use assert_approx_eq::assert_approx_eq;
    /// # use tune::note::NoteLetter;
    /// # use tune::pitch::Pitch;
    /// # use tune::tuning::ConcertPitch;
    /// use tune::pitch::Pitched;
    ///
    /// let a4 = NoteLetter::A.in_octave(4);
    /// let tuning = ConcertPitch::from_a4_pitch(Pitch::from_hz(432.0));
    ///
    /// let approximation = a4.find_in_tuning(tuning);
    /// assert_eq!(approximation.approx_value, a4);
    /// assert_approx_eq!(approximation.deviation.as_cents(), 31.766654);
    /// ```
    fn find_in_tuning<K, T: Tuning<K>>(self, tuning: T) -> Approximation<K> {
        tuning.find_by_pitch(self.pitch())
    }
}

impl Pitched for Pitch {
    fn pitch(self) -> Pitch {
        self
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ReferencePitch {
    key: PianoKey,
    pitch: Pitch,
}

impl ReferencePitch {
    pub fn from_note(note: impl PitchedNote) -> Self {
        Self::from_key_and_pitch(note.note().as_piano_key(), note)
    }

    pub fn from_key_and_pitch(key: PianoKey, pitched: impl Pitched) -> Self {
        Self {
            key,
            pitch: pitched.pitch(),
        }
    }

    pub fn key(&self) -> PianoKey {
        self.key
    }

    pub fn pitch(&self) -> Pitch {
        self.pitch
    }
}

impl FromStr for ReferencePitch {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let [note, pitch] = parse::split_balanced(s, '@').as_slice() {
            let note_number = note
                .parse::<i32>()
                .map_err(|_| format!("Invalid note '{}': Must be an integer", note))?;
            let pitch: Pitch = pitch
                .parse()
                .map_err(|e| format!("Invalid pitch '{}': {}", pitch, e))?;
            Ok(ReferencePitch::from_key_and_pitch(
                PianoKey::from_midi_number(note_number),
                pitch,
            ))
        } else if let [note, delta] = parse::split_balanced(s, '+').as_slice() {
            let note_number = note
                .parse::<i32>()
                .map_err(|_| format!("Invalid note '{}': Must be an integer", note))?;
            let delta = delta
                .parse()
                .map_err(|e| format!("Invalid delta '{}': {}", delta, e))?;
            Ok(ReferencePitch::from_note(
                Note::from_midi_number(note_number).alter_pitch_by(delta),
            ))
        } else if let [note, delta] = parse::split_balanced(s, '-').as_slice() {
            let note_number = note
                .parse::<i32>()
                .map_err(|_| format!("Invalid note '{}': Must be an integer", note))?;
            let delta = delta
                .parse::<Ratio>()
                .map_err(|e| format!("Invalid delta '{}': {}", delta, e))?;
            Ok(ReferencePitch::from_note(
                Note::from_midi_number(note_number).alter_pitch_by(delta.inv()),
            ))
        } else {
            let note_number = s
                .parse::<i32>()
                .map_err(|_| "Must be an expression of type 69, 69@440Hz or 69+100c".to_string())?;
            Ok(ReferencePitch::from_note(Note::from_midi_number(
                note_number,
            )))
        }
    }
}
