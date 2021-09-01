mod assets;
mod audio;
mod fluid;
mod keypress;
mod magnetron;
mod midi;
mod model;
mod piano;
mod synth;
mod view;

use std::{io, path::PathBuf, process, sync::mpsc};

use audio::{AudioModel, AudioOptions};
use magnetron::effects::{DelayOptions, ReverbOptions, RotaryOptions};
use midi::TuningMethod;
use model::{Model, SourceId};
use nannou::app::App;
use piano::{Backend, NoAudio, PianoEngine};
use structopt::StructOpt;
use synth::ControlChangeNumbers;
use tune::{
    key::{Keyboard, PianoKey},
    note::NoteLetter,
    pitch::Ratio,
    scala::{Kbm, Scl},
    temperament::{EqualTemperament, TemperamentPreference},
};
use tune_cli::{
    shared::{self, KbmOptions, SclCommand},
    CliError, CliResult,
};
use view::DynViewModel;

#[derive(StructOpt)]
enum MainCommand {
    /// Start the microwave GUI
    #[structopt(name = "run")]
    Run(RunOptions),

    /// Use a keyboard mapping with the given reference note
    #[structopt(name = "ref-note")]
    WithRefNote {
        #[structopt(flatten)]
        kbm: KbmOptions,

        #[structopt(flatten)]
        options: RunOptions,
    },

    /// Use a kbm file
    #[structopt(name = "kbm-file")]
    UseKbmFile {
        /// The location of the kbm file to import
        kbm_file_location: PathBuf,

        #[structopt(flatten)]
        options: RunOptions,
    },

    /// List MIDI devices
    #[structopt(name = "devices")]
    Devices,
}

#[derive(StructOpt)]
struct RunOptions {
    /// MIDI target device
    #[structopt(long = "midi-out")]
    midi_target: Option<String>,

    /// MIDI-out tuning method
    #[structopt(long = "tun-method", default_value="full", parse(try_from_str=parse_tuning_method))]
    midi_tuning_method: TuningMethod,

    /// MIDI source device
    #[structopt(long = "midi-in")]
    midi_source: Option<String>,

    /// MIDI channel (0-based) to listen to
    #[structopt(long = "in-chan", default_value = "0")]
    midi_channel: u8,

    /// Waveforms file location (waveform synth)
    #[structopt(
        long = "wv-loc",
        env = "MICROWAVE_WV_LOC",
        default_value = "waveforms.yml"
    )]
    waveforms_file_location: PathBuf,

    /// Number of waveform buffers to allocate
    #[structopt(long = "wv-bufs", default_value = "8")]
    num_waveform_buffers: usize,

    #[structopt(flatten)]
    control_change: ControlChangeParameters,

    /// Pitch wheel sensitivity (waveform synth)
    #[structopt(long = "pwsens", default_value = "200c")]
    pitch_wheel_sensitivity: Ratio,

    /// Enable logging
    #[structopt(long = "log")]
    logging: bool,

    /// Enable fluidlite using the soundfont file at the given location
    #[structopt(long = "sf-loc", env = "MICROWAVE_SF_LOC")]
    soundfont_file_location: Option<PathBuf>,

    #[structopt(flatten)]
    audio: AudioParameters,

    #[structopt(flatten)]
    reverb: ReverbParameters,

    #[structopt(flatten)]
    delay: DelayParameters,

    #[structopt(flatten)]
    rotary: RotaryParameters,

    /// Program number that should be selected at startup
    #[structopt(long = "pg", default_value = "0")]
    program_number: u8,

    /// Use porcupine layout when possible
    #[structopt(long = "porcupine")]
    use_porcupine: bool,

    /// Primary step width (right direction) when playing on the computer keyboard
    #[structopt(long = "ps")]
    primary_step: Option<i16>,

    /// Secondary step width (down/right direction) when playing on the computer keyboard
    #[structopt(long = "ss")]
    secondary_step: Option<i16>,

    /// Odd limit for frequency ratio indicators
    #[structopt(long = "lim", default_value = "11")]
    odd_limit: u16,

    #[structopt(subcommand)]
    scl: Option<SclCommand>,
}

#[derive(StructOpt)]
struct ControlChangeParameters {
    /// Modulation control number (MIDI -> waveform synth)
    #[structopt(long = "modulation-ccn", default_value = "1")]
    modulation_ccn: u8,

    /// Breath control number (MIDI -> waveform synth)
    #[structopt(long = "breath-ccn", default_value = "2")]
    breath_ccn: u8,

    /// Foot control number (MIDI -> waveform synth)
    #[structopt(long = "foot-ccn", default_value = "4")]
    foot_ccn: u8,

    /// Expression control number (MIDI -> waveform synth)
    #[structopt(long = "expression-ccn", default_value = "11")]
    expression_ccn: u8,

    /// Damper pedal control number (MIDI -> waveform synth)
    #[structopt(long = "damper-ccn", default_value = "64")]
    damper_ccn: u8,

    /// Sostenuto pedal control number (MIDI -> waveform synth)
    #[structopt(long = "sostenuto-ccn", default_value = "66")]
    sostenuto_ccn: u8,

    /// Soft pedal control number (MIDI -> waveform synth)
    #[structopt(long = "soft-ccn", default_value = "67")]
    soft_ccn: u8,

    /// Mouse Y control number (microwave GUI -> MIDI)
    #[structopt(long = "mouse-ccn", default_value = "2")]
    mouse_y_ccn: u8,
}

#[derive(StructOpt)]
struct AudioParameters {
    /// Enable audio-in
    #[structopt(long = "audio-in")]
    audio_in_enabled: bool,

    /// Audio-out buffer size in frames
    #[structopt(long = "out-buf", default_value = "1024")]
    out_buffer_size: u32,

    /// Audio-in buffer size in frames
    #[structopt(long = "in-buf", default_value = "1024")]
    in_buffer_size: u32,

    /// Size of the ring buffer piping data from audio-in to audio-out in frames
    #[structopt(long = "exc-buf", default_value = "8192")]
    exchange_buffer_size: usize,

    /// Prefix for wav file recordings
    #[structopt(long = "wav-prefix", default_value = "microwave")]
    wav_file_prefix: String,
}

#[derive(StructOpt)]
struct ReverbParameters {
    /// Short-response diffusing delay lines (ms)
    #[structopt(
        long = "rev-aps",
        require_delimiter = true,
        default_value = "5.10,7.73,10.00,12.61"
    )]
    reverb_allpasses: Vec<f64>,

    /// Short-response diffuse feedback
    #[structopt(long = "rev-ap-fb", default_value = "0.5")]
    reverb_allpass_feedback: f64,

    /// Long-response resonating delay lines (ms)
    #[structopt(
        long = "rev-combs",
        require_delimiter = true,
        default_value = "25.31,26.94,28.96,30.75,32.24,33.81,35.31,36.67"
    )]
    reverb_combs: Vec<f64>,

    /// Long-response resonant feedback
    #[structopt(long = "rev-comb-fb", default_value = "0.95")]
    reverb_comb_feedback: f64,

    /// Long-response damping cutoff (Hz)
    #[structopt(long = "rev-cutoff", default_value = "5600.0")]
    reverb_cutoff: f64,

    /// Additional delay (ms) on right channel for an enhanced stereo effect
    #[structopt(long = "rev-stereo", default_value = "0.52")]
    reverb_stereo: f64,

    /// Balance between original and reverbed signal (0.0 = original signal only, 1.0 = reverbed signal only)
    #[structopt(long = "rev-wet", default_value = "0.5")]
    reverb_wetness: f64,
}

#[derive(StructOpt)]
struct DelayParameters {
    /// Delay time (s)
    #[structopt(long = "del-tm", default_value = "0.5")]
    delay_time: f64,

    /// Delay feedback
    #[structopt(long = "del-fb", default_value = "0.6")]
    delay_feedback: f64,

    /// Delay feedback rotation angle (degrees clock-wise)
    #[structopt(long = "del-rot", default_value = "135")]
    delay_feedback_rotation: f64,
}

#[derive(StructOpt)]
struct RotaryParameters {
    /// Rotary speaker radius (cm)
    #[structopt(long = "rot-rad", default_value = "20")]
    rotation_radius: f64,

    /// Rotary speaker minimum speed (revolutions per s)
    #[structopt(long = "rot-min", default_value = "1")]
    rotation_min_frequency: f64,

    /// Rotary speaker maximum speed (revolutions per s)
    #[structopt(long = "rot-max", default_value = "7")]
    rotation_max_frequency: f64,

    /// Rotary speaker acceleration time (s)
    #[structopt(long = "rot-acc", default_value = "1")]
    rotation_acceleration: f64,

    /// Rotary speaker deceleration time (s)
    #[structopt(long = "rot-dec", default_value = "0.5")]
    rotation_deceleration: f64,
}

fn parse_tuning_method(src: &str) -> Result<TuningMethod, &'static str> {
    Ok(match &*src.to_lowercase() {
        "full" => TuningMethod::FullKeyboard,
        "octave" => TuningMethod::Octave,
        "channel" => TuningMethod::ChannelFineTuning,
        "pitch-bend" => TuningMethod::PitchBend,
        _ => {
            return Err(
                "Invalid tuning method. Should be `full`, `octave`, `channel` or `pitch-bend`",
            )
        }
    })
}

fn main() {
    nannou::app(model).update(model::update).run();
}

fn model(app: &App) -> Model {
    let model = match MainCommand::from_args() {
        MainCommand::Run(options) => Kbm::builder(NoteLetter::D.in_octave(4))
            .build()
            .map_err(CliError::from)
            .and_then(|kbm| create_model(kbm, options)),
        MainCommand::WithRefNote { kbm, options } => kbm
            .to_kbm()
            .map_err(CliError::from)
            .and_then(|kbm| create_model(kbm, options)),
        MainCommand::UseKbmFile {
            kbm_file_location,
            options,
        } => shared::import_kbm_file(&kbm_file_location)
            .map_err(CliError::from)
            .and_then(|kbm| create_model(kbm, options)),
        MainCommand::Devices => {
            let stdout = io::stdout();
            shared::print_midi_devices(stdout.lock(), "microwave").unwrap();
            process::exit(1);
        }
    };
    match model {
        Ok(model) => {
            create_window(app);
            model
        }
        Err(err) => {
            eprintln!("{:?}", err);
            process::exit(1);
        }
    }
}

fn create_model(kbm: Kbm, options: RunOptions) -> CliResult<Model> {
    let scl = options
        .scl
        .as_ref()
        .map(|command| command.to_scl(None))
        .transpose()
        .map_err(|x| format!("error ({:?})", x))?
        .unwrap_or_else(|| {
            Scl::builder()
                .push_ratio(Ratio::from_semitones(1))
                .build()
                .unwrap()
        });

    let keyboard = create_keyboard(&scl, &options);

    let (send, recv) = mpsc::channel::<DynViewModel>();

    let mut backends = Vec::<Box<dyn Backend<SourceId>>>::new();

    if let Some(target_port) = options.midi_target {
        let midi_backend = midi::create(send.clone(), &target_port, options.midi_tuning_method)?;
        backends.push(Box::new(midi_backend));
    }

    let (fluid_backend, fluid_synth) =
        fluid::create(send.clone(), options.soundfont_file_location.as_deref());
    if options.soundfont_file_location.is_some() {
        backends.push(Box::new(fluid_backend));
    }

    let (waveform_backend, waveform_synth) = synth::create(
        send.clone(),
        &options.waveforms_file_location,
        options.pitch_wheel_sensitivity,
        options.control_change.to_cc_numbers(),
        options.num_waveform_buffers,
        options.audio.out_buffer_size as usize,
    )?;
    backends.push(Box::new(waveform_backend));
    backends.push(Box::new(NoAudio::new(send)));

    let (engine, engine_snapshot) = PianoEngine::new(scl, kbm, backends, options.program_number);

    let audio = AudioModel::new(
        fluid_synth,
        waveform_synth,
        options.audio.into_options(),
        options.reverb.into_options(),
        options.delay.to_options(),
        options.rotary.to_options(),
    );

    let (midi_channel, midi_logging) = (options.midi_channel, options.logging);
    let midi_in = options
        .midi_source
        .map(|midi_source| {
            midi::connect_to_midi_device(&midi_source, engine.clone(), midi_channel, midi_logging)
        })
        .transpose()?
        .map(|(_, connection)| connection);

    let mut model = Model::new(
        audio,
        engine,
        engine_snapshot,
        keyboard,
        options.odd_limit,
        midi_in,
        options.control_change.mouse_y_ccn,
        recv,
    );
    model.toggle_reverb();
    Ok(model)
}

fn create_keyboard(scl: &Scl, config: &RunOptions) -> Keyboard {
    let preference = if config.use_porcupine {
        TemperamentPreference::Porcupine
    } else {
        TemperamentPreference::PorcupineWhenMeantoneIsBad
    };

    let average_step_size = scl.period().divided_into_equal_steps(scl.num_items());

    let temperament = EqualTemperament::find()
        .with_preference(preference)
        .by_step_size(average_step_size);

    let keyboard = Keyboard::root_at(PianoKey::from_midi_number(0))
        .with_steps_of(&temperament)
        .coprime();

    let primary_step = config
        .primary_step
        .unwrap_or_else(|| keyboard.primary_step());
    let secondary_step = config
        .secondary_step
        .unwrap_or_else(|| keyboard.secondary_step());

    keyboard.with_steps(primary_step, secondary_step)
}

fn create_window(app: &App) {
    app.new_window()
        .maximized(true)
        .title("Microwave - Microtonal Waveform Synthesizer by Woyten")
        .raw_event(model::raw_event)
        .key_pressed(model::key_pressed)
        .mouse_pressed(model::mouse_pressed)
        .mouse_moved(model::mouse_moved)
        .mouse_released(model::mouse_released)
        .mouse_wheel(model::mouse_wheel)
        .touch(model::touch)
        .view(view::view)
        .build()
        .unwrap();
}

impl ControlChangeParameters {
    fn to_cc_numbers(&self) -> ControlChangeNumbers {
        ControlChangeNumbers {
            modulation: self.modulation_ccn,
            breath: self.breath_ccn,
            foot: self.foot_ccn,
            expression: self.expression_ccn,
            damper: self.damper_ccn,
            sostenuto: self.sostenuto_ccn,
            soft: self.soft_ccn,
        }
    }
}

impl AudioParameters {
    fn into_options(self) -> AudioOptions {
        AudioOptions {
            audio_in_enabled: self.audio_in_enabled,
            output_buffer_size: self.out_buffer_size,
            input_buffer_size: self.in_buffer_size,
            exchange_buffer_size: self.exchange_buffer_size,
            wav_file_prefix: self.wav_file_prefix,
        }
    }
}

impl ReverbParameters {
    fn into_options(self) -> ReverbOptions {
        ReverbOptions {
            allpasses_ms: self.reverb_allpasses,
            allpass_feedback: self.reverb_allpass_feedback,
            combs_ms: self.reverb_combs,
            comb_feedback: self.reverb_comb_feedback,
            stereo_ms: self.reverb_stereo,
            cutoff_hz: self.reverb_cutoff,
            wetness: self.reverb_wetness,
        }
    }
}

impl DelayParameters {
    fn to_options(&self) -> DelayOptions {
        DelayOptions {
            delay_time_in_s: self.delay_time,
            feedback_intensity: self.delay_feedback,
            feedback_rotation: self.delay_feedback_rotation.to_radians(),
        }
    }
}

impl RotaryParameters {
    fn to_options(&self) -> RotaryOptions {
        RotaryOptions {
            rotation_radius_in_cm: self.rotation_radius,
            min_frequency_in_hz: self.rotation_min_frequency,
            max_frequency_in_hz: self.rotation_max_frequency,
            acceleration_time_in_s: self.rotation_acceleration,
            deceleration_time_in_s: self.rotation_deceleration,
        }
    }
}
