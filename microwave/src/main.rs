mod app;
mod assets;
mod audio;
mod backend;
mod bench;
mod control;
mod fluid;
mod keypress;
mod magnetron;
mod midi;
mod piano;
mod portable;
mod profile;
mod synth;
#[cfg(test)]
mod test;
mod tunable;

use std::{io, path::PathBuf, str::FromStr};

use ::magnetron::creator::Creator;
use async_std::task;
use clap::Parser;
use control::{LiveParameter, LiveParameterMapper, LiveParameterStorage, ParameterValue};
use crossbeam::channel;
use log::{error, warn};
use piano::PianoEngine;
use profile::MicrowaveProfile;
use tune::{
    key::{Keyboard, PianoKey},
    note::NoteLetter,
    pitch::Ratio,
    scala::{Kbm, Scl},
    temperament::{EqualTemperament, TemperamentPreference},
};
use tune_cli::{
    shared::{self, midi::MidiInArgs, KbmOptions, SclCommand},
    CliResult,
};

#[derive(Parser)]
#[command(version)]
enum MainOptions {
    /// Start the microwave GUI
    #[command(name = "run")]
    Run(RunOptions),

    /// Use a keyboard mapping with the given reference note
    #[command(name = "ref-note")]
    WithRefNote {
        #[command(flatten)]
        kbm: KbmOptions,

        #[command(flatten)]
        options: RunOptions,
    },

    /// Use a kbm file
    #[command(name = "kbm-file")]
    UseKbmFile {
        /// The location of the kbm file to import
        kbm_file_location: PathBuf,

        #[command(flatten)]
        options: RunOptions,
    },

    /// List MIDI devices
    #[command(name = "devices")]
    Devices,

    /// Run benchmark
    #[command(name = "bench")]
    Bench {
        /// Analyze benchmark
        #[arg(long = "analyze")]
        analyze: bool,
    },
}

#[derive(Parser)]
struct RunOptions {
    /// MIDI input device
    #[arg(long = "midi-in")]
    midi_in_device: Option<String>,

    #[command(flatten)]
    midi_in_args: MidiInArgs,

    /// Profile location
    #[arg(
        short = 'p',
        long = "profile",
        env = "MICROWAVE_PROFILE",
        default_value = "microwave.yml"
    )]
    profile_location: String,

    #[command(flatten)]
    control_change: ControlChangeParameters,

    /// Enable logging
    #[arg(long = "log")]
    logging: bool,

    #[command(flatten)]
    audio: AudioParameters,

    /// Program number that should be selected at startup
    #[arg(long = "pg", default_value = "0")]
    program_number: u8,

    /// Use porcupine layout when possible
    #[arg(long = "porcupine")]
    use_porcupine: bool,

    /// Primary step width (right direction) when playing on the computer keyboard
    #[arg(long = "p-step")]
    primary_step: Option<i16>,

    /// Secondary step width (down/right direction) when playing on the computer keyboard
    #[arg(long = "s-step")]
    secondary_step: Option<i16>,

    /// Physical keyboard layout.
    /// [ansi] Large backspace key, horizontal enter key, large left shift key.
    /// [var] Subdivided backspace key, large enter key, large left shift key.
    /// [iso] Large backspace key, vertical enter key, subdivided left shift key.
    #[arg(long = "keyb", default_value = "iso")]
    keyboard_layout: KeyboardLayout,

    /// Odd limit for frequency ratio indicators
    #[arg(long = "lim", default_value = "11")]
    odd_limit: u16,

    /// Color schema of the scale-specific keyboard (e.g. wgrwwgrwgrwgrwwgr for 17-EDO)
    #[arg(long = "kb2", default_value = "wbwwbwbwbwwb", value_parser = parse_key_colors)]
    scale_keyboard_colors: KeyColors,

    #[command(subcommand)]
    scl: Option<SclCommand>,
}

#[derive(Parser)]
struct ControlChangeParameters {
    /// Modulation control number - generic controller
    #[arg(long = "modulation-ccn", default_value = "1")]
    modulation_ccn: u8,

    /// Breath control number - triggered by vertical mouse movement
    #[arg(long = "breath-ccn", default_value = "2")]
    breath_ccn: u8,

    /// Foot switch control number - controls recording
    #[arg(long = "foot-ccn", default_value = "4")]
    foot_ccn: u8,

    /// Volume control number - generic controller
    #[arg(long = "volume-ccn", default_value = "7")]
    volume_ccn: u8,

    /// Balance control number - generic controller
    #[arg(long = "balance-ccn", default_value = "8")]
    balance_ccn: u8,

    /// Panning control number - generic controller
    #[arg(long = "pan-ccn", default_value = "10")]
    pan_ccn: u8,

    /// Expression control number - generic controller
    #[arg(long = "expression-ccn", default_value = "11")]
    expression_ccn: u8,

    /// Damper pedal control number - generic controller
    #[arg(long = "damper-ccn", default_value = "64")]
    damper_ccn: u8,

    /// Sostenuto pedal control number - generic controller
    #[arg(long = "sostenuto-ccn", default_value = "66")]
    sostenuto_ccn: u8,

    /// Soft pedal control number - generic controller
    #[arg(long = "soft-ccn", default_value = "67")]
    soft_ccn: u8,

    /// Legato switch control number - controls legato option
    #[arg(long = "legato-ccn", default_value = "68")]
    legato_ccn: u8,

    /// Sound 1 control number. Triggered by F1 key
    #[arg(long = "sound-1-ccn", default_value = "70")]
    sound_1_ccn: u8,

    /// Sound 2 control number. Triggered by F2 key
    #[arg(long = "sound-2-ccn", default_value = "71")]
    sound_2_ccn: u8,

    /// Sound 3 control number. Triggered by F3 key
    #[arg(long = "sound-3-ccn", default_value = "72")]
    sound_3_ccn: u8,

    /// Sound 4 control number. Triggered by F4 key
    #[arg(long = "sound-4-ccn", default_value = "73")]
    sound_4_ccn: u8,

    /// Sound 5 control number. Triggered by F5 key
    #[arg(long = "sound-5-ccn", default_value = "74")]
    sound_5_ccn: u8,

    /// Sound 6 control number. Triggered by F6 key
    #[arg(long = "sound-6-ccn", default_value = "75")]
    sound_6_ccn: u8,

    /// Sound 7 control number. Triggered by F7 key
    #[arg(long = "sound-7-ccn", default_value = "76")]
    sound_7_ccn: u8,

    /// Sound 8 control number. Triggered by F8 key
    #[arg(long = "sound-8-ccn", default_value = "77")]
    sound_8_ccn: u8,

    /// Sound 9 control number. Triggered by F9 key
    #[arg(long = "sound-9-ccn", default_value = "78")]
    sound_9_ccn: u8,

    /// Sound 10 control number. Triggered by F10 key
    #[arg(long = "sound-10-ccn", default_value = "79")]
    sound_10_ccn: u8,
}

#[derive(Parser)]
struct AudioParameters {
    /// Audio-out buffer size in frames
    #[arg(long = "out-buf", default_value = "1024")]
    buffer_size: u32,

    /// Sample rate [Hz]. If no value is specified the audio device's preferred value will be used
    #[arg(long = "s-rate")]
    sample_rate: Option<u32>,

    /// Prefix for wav file recordings
    #[arg(long = "wav-prefix", default_value = "microwave")]
    wav_file_prefix: String,
}

#[derive(Clone, Copy)]
pub enum KeyboardLayout {
    Ansi,
    Variant,
    Iso,
}

impl FromStr for KeyboardLayout {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const ANSI: &str = "ansi";
        const VAR: &str = "var";
        const ISO: &str = "iso";

        match s {
            ANSI => Ok(Self::Ansi),
            VAR => Ok(Self::Variant),
            ISO => Ok(Self::Iso),
            _ => Err(format!(
                "Invalid keyboard layout. Should be `{ANSI}`, `{VAR}` or `{ISO}`."
            )),
        }
    }
}

#[derive(Clone)]
struct KeyColors(Vec<KeyColor>);

#[derive(Clone, Copy)]
pub enum KeyColor {
    White,
    Red,
    Green,
    Blue,
    Cyan,
    Magenta,
    Yellow,
    Black,
}

fn parse_key_colors(src: &str) -> Result<KeyColors, String> {
    src.chars()
        .map(|c| match c {
            'w' => Ok(KeyColor::White),
            'r' => Ok(KeyColor::Red),
            'g' => Ok(KeyColor::Green),
            'b' => Ok(KeyColor::Blue),
            'c' => Ok(KeyColor::Cyan),
            'm' => Ok(KeyColor::Magenta),
            'y' => Ok(KeyColor::Yellow),
            'k' => Ok(KeyColor::Black),
            c => Err(format!(
                "Received an invalid character '{c}'. Only wrgbcmyk are allowed."
            )),
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(|key_colors| {
            if key_colors.is_empty() {
                Err("Color schema must not be empty".to_owned())
            } else {
                Ok(KeyColors(key_colors))
            }
        })
}

fn main() {
    portable::init_environment();

    let args = portable::get_args();

    let options = if args.len() < 2 {
        let executable_name = &args[0];
        warn!("Use a subcommand, e.g. `{executable_name} run` to start microwave properly");
        MainOptions::parse_from([executable_name, "run"])
    } else {
        MainOptions::parse_from(&args)
    };

    task::block_on(async {
        if let Err(err) = run_from_main_options(options).await {
            error!("{err:?}");
        }
    })
}

async fn run_from_main_options(options: MainOptions) -> CliResult {
    match options {
        MainOptions::Run(options) => {
            run_from_run_options(Kbm::builder(NoteLetter::D.in_octave(4)).build()?, options).await
        }
        MainOptions::WithRefNote { kbm, options } => {
            run_from_run_options(kbm.to_kbm()?, options).await
        }
        MainOptions::UseKbmFile {
            kbm_file_location,
            options,
        } => run_from_run_options(shared::import_kbm_file(&kbm_file_location)?, options).await,
        MainOptions::Devices => {
            let stdout = io::stdout();
            Ok(shared::midi::print_midi_devices(
                stdout.lock(),
                "microwave",
            )?)
        }
        MainOptions::Bench { analyze } => {
            if analyze {
                bench::analyze_benchmark()
            } else {
                bench::run_benchmark()
            }
        }
    }
}

async fn run_from_run_options(kbm: Kbm, options: RunOptions) -> CliResult {
    let scl = options
        .scl
        .as_ref()
        .map(|command| command.to_scl(None))
        .transpose()
        .map_err(|x| format!("error ({x:?})"))?
        .unwrap_or_else(|| {
            Scl::builder()
                .push_ratio(Ratio::from_semitones(1))
                .build()
                .unwrap()
        });

    let keyboard = create_keyboard(&scl, &options);

    let output_stream_params =
        audio::get_output_stream_params(options.audio.buffer_size, options.audio.sample_rate);

    let profile = MicrowaveProfile::load(&options.profile_location).await?;

    let waveform_templates = profile
        .waveform_templates
        .into_iter()
        .map(|spec| (spec.name, spec.value))
        .collect();

    let waveform_envelopes = profile
        .waveform_envelopes
        .into_iter()
        .map(|spec| (spec.name, spec.spec))
        .collect();

    let effect_templates = profile
        .effect_templates
        .into_iter()
        .map(|spec| (spec.name, spec.value))
        .collect();

    let creator = Creator::new(effect_templates);

    let (info_send, info_recv) = channel::unbounded();

    let mut backends = Vec::new();
    let mut stages = Vec::new();
    let mut resources = Vec::new();

    for stage in profile.stages {
        stage
            .create(
                &creator,
                options.audio.buffer_size,
                output_stream_params.1.sample_rate,
                &info_send,
                &waveform_templates,
                &waveform_envelopes,
                &mut backends,
                &mut stages,
                &mut resources,
            )
            .await?;
    }

    let mut storage = LiveParameterStorage::default();
    storage.set_parameter(LiveParameter::Volume, 100u8.as_f64());
    storage.set_parameter(LiveParameter::Balance, 0.5);
    storage.set_parameter(LiveParameter::Pan, 0.5);
    storage.set_parameter(LiveParameter::Legato, 1.0);

    let (storage_send, storage_recv) = channel::unbounded();

    let (engine, engine_state) = PianoEngine::new(
        scl,
        kbm,
        backends,
        options.program_number,
        options.control_change.to_parameter_mapper(),
        storage,
        storage_send,
    );

    resources.push(Box::new(audio::start_context(
        output_stream_params,
        options.audio.buffer_size,
        profile.num_buffers,
        profile.audio_buffers,
        stages,
        options.audio.wav_file_prefix,
        storage,
        storage_recv,
    )));

    options
        .midi_in_device
        .map(|midi_in_device| {
            midi::connect_to_midi_device(
                engine.clone(),
                &midi_in_device,
                options.midi_in_args,
                options.logging,
            )
            .map(|(_, connection)| resources.push(Box::new(connection)))
        })
        .transpose()?;

    app::start(
        engine,
        engine_state,
        options.scale_keyboard_colors.0,
        keyboard,
        options.keyboard_layout,
        options.odd_limit,
        info_recv,
        resources,
    );

    Ok(())
}

fn create_keyboard(scl: &Scl, options: &RunOptions) -> Keyboard {
    let preference = if options.use_porcupine {
        TemperamentPreference::Porcupine
    } else {
        TemperamentPreference::PorcupineWhenMeantoneIsBad
    };

    let average_step_size = if scl.period().is_negligible() {
        Ratio::from_octaves(1)
    } else {
        scl.period()
    }
    .divided_into_equal_steps(scl.num_items());

    let temperament = EqualTemperament::find()
        .with_preference(preference)
        .by_step_size(average_step_size);

    let keyboard = Keyboard::root_at(PianoKey::from_midi_number(0))
        .with_steps_of(&temperament)
        .coprime();

    let primary_step = options
        .primary_step
        .unwrap_or_else(|| keyboard.primary_step());
    let secondary_step = options
        .secondary_step
        .unwrap_or_else(|| keyboard.secondary_step());

    keyboard.with_steps(primary_step, secondary_step)
}

impl ControlChangeParameters {
    fn to_parameter_mapper(&self) -> LiveParameterMapper {
        let mut mapper = LiveParameterMapper::new();
        mapper.push_mapping(LiveParameter::Modulation, self.modulation_ccn);
        mapper.push_mapping(LiveParameter::Breath, self.breath_ccn);
        mapper.push_mapping(LiveParameter::Foot, self.foot_ccn);
        mapper.push_mapping(LiveParameter::Volume, self.volume_ccn);
        mapper.push_mapping(LiveParameter::Balance, self.balance_ccn);
        mapper.push_mapping(LiveParameter::Pan, self.pan_ccn);
        mapper.push_mapping(LiveParameter::Expression, self.expression_ccn);
        mapper.push_mapping(LiveParameter::Damper, self.damper_ccn);
        mapper.push_mapping(LiveParameter::Sostenuto, self.sostenuto_ccn);
        mapper.push_mapping(LiveParameter::Soft, self.soft_ccn);
        mapper.push_mapping(LiveParameter::Legato, self.legato_ccn);
        mapper.push_mapping(LiveParameter::Sound1, self.sound_1_ccn);
        mapper.push_mapping(LiveParameter::Sound2, self.sound_2_ccn);
        mapper.push_mapping(LiveParameter::Sound3, self.sound_3_ccn);
        mapper.push_mapping(LiveParameter::Sound4, self.sound_4_ccn);
        mapper.push_mapping(LiveParameter::Sound5, self.sound_5_ccn);
        mapper.push_mapping(LiveParameter::Sound6, self.sound_6_ccn);
        mapper.push_mapping(LiveParameter::Sound7, self.sound_7_ccn);
        mapper.push_mapping(LiveParameter::Sound8, self.sound_8_ccn);
        mapper.push_mapping(LiveParameter::Sound9, self.sound_9_ccn);
        mapper.push_mapping(LiveParameter::Sound10, self.sound_10_ccn);
        mapper
    }
}
