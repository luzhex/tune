mod dto;

use dto::{DumpDto, DumpItemDto, TuneDto};
use io::ErrorKind;
use scale::ScaleWithKeyMap;
use std::fmt::Display;
use std::fs::File;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use structopt::StructOpt;
use tune::key::PianoKey;
use tune::key_map::KeyMap;
use tune::mts::SingleNoteTuningChangeMessage;
use tune::pitch::{Pitch, ReferencePitch};
use tune::ratio::Ratio;
use tune::scale;
use tune::scale::Scale;
use tune::{
    note::Note,
    tuning::{Approximation, ConcertPitch, Tuning},
};

#[derive(StructOpt)]
enum Options {
    /// Create a scale file
    #[structopt(name = "scl")]
    Scale(ScaleOptions),

    /// Create a keyboard mapping file
    #[structopt(name = "kbm")]
    KeyMap(KeyMapOptions),

    /// Dump pitches of a scale
    #[structopt(name = "dump")]
    Dump(DumpOptions),

    /// Dump pitches of a scale in JSON format
    #[structopt(name = "jdump")]
    JsonDump(JsonDumpOptions),

    /// Display differences between a source scale and a target scale
    #[structopt(name = "diff")]
    Diff(DiffOptions),

    // Dump MIDI tuning messages
    #[structopt(name = "mts")]
    Mts(MtsOptions),
}

#[derive(StructOpt)]
struct ScaleOptions {
    #[structopt(flatten)]
    output_file_params: OutputFileParams,

    #[structopt(subcommand)]
    command: ScaleCommand,
}

#[derive(StructOpt)]
struct KeyMapOptions {
    #[structopt(flatten)]
    output_file_params: OutputFileParams,

    #[structopt(flatten)]
    key_map_params: KeyMapParams,
}

#[derive(StructOpt)]
struct DumpOptions {
    #[structopt(flatten)]
    key_map_params: KeyMapParams,

    #[structopt(flatten)]
    limit_params: LimitParams,

    #[structopt(subcommand)]
    command: ScaleCommand,
}

#[derive(StructOpt)]
struct JsonDumpOptions {
    #[structopt(flatten)]
    key_map_params: KeyMapParams,

    #[structopt(subcommand)]
    command: ScaleCommand,
}

#[derive(StructOpt)]
struct DiffOptions {
    #[structopt(flatten)]
    key_map_params: KeyMapParams,

    #[structopt(flatten)]
    limit_params: LimitParams,

    #[structopt(subcommand)]
    command: ScaleCommand,
}

#[derive(StructOpt)]
struct MtsOptions {
    #[structopt(flatten)]
    key_map_params: KeyMapParams,

    #[structopt(subcommand)]
    command: ScaleCommand,
}

#[derive(StructOpt)]
enum ScaleCommand {
    /// Equal temperament
    #[structopt(name = "equal")]
    EqualTemperament {
        /// Step size, e.g. 1:12:2
        step_size: Ratio,
    },

    /// Rank-2 temperament
    #[structopt(name = "rank2")]
    Rank2Temperament {
        /// First generator (finite), e.g. 3/2
        generator: Ratio,

        /// Number of positive generations using the first generator, e.g. 6
        num_pos_generations: u16,

        /// Number of negative generations using the first generator, e.g. 1
        #[structopt(default_value = "0")]
        num_neg_generations: u16,

        /// Second generator (infinite)
        #[structopt(short = "p", default_value = "2")]
        period: Ratio,
    },

    /// Harmonic series
    #[structopt(name = "harm")]
    HarmonicSeries {
        /// The lowest harmonic, e.g. 8
        lowest_harmonic: u16,

        /// Number of of notes, e.g. 8
        #[structopt(short = "n")]
        number_of_notes: Option<u16>,

        /// Build subharmonic series
        #[structopt(short = "s")]
        subharmonics: bool,
    },

    /// Custom Scale
    #[structopt(name = "cust")]
    Custom {
        /// Items of the scale
        items: Vec<Ratio>,

        /// Name of the scale
        #[structopt(short = "n")]
        name: Option<String>,
    },
}

#[derive(StructOpt)]
struct OutputFileParams {
    /// Write output to file
    #[structopt(short = "o")]
    output_file: Option<PathBuf>,
}

#[derive(StructOpt)]
struct KeyMapParams {
    /// Reference note that should sound at its original or a custom pitch, e.g. 69@440Hz
    ref_pitch: ReferencePitch,

    /// root note / "middle note" of the scale if different from reference note
    #[structopt(short = "r")]
    root_note: Option<i16>,
}

#[derive(StructOpt)]
struct LimitParams {
    /// Largest acceptable numerator or denominator (ignoring powers of two)
    #[structopt(short = "l", default_value = "11")]
    limit: u16,
}

fn main() -> io::Result<()> {
    match try_main() {
        // The BrokenPipe case occurs when stdout tries to communicate with a process that has already terminated.
        // Since tune is an idempotent tool with repeatable results, it is okay to ignore this error and terminate successfully.
        Err(err) if err.kind() == ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

fn try_main() -> io::Result<()> {
    match Options::from_args() {
        Options::Scale(ScaleOptions {
            output_file_params,
            command,
        }) => execute_scale_command(output_file_params, command),
        Options::KeyMap(KeyMapOptions {
            output_file_params,
            key_map_params,
        }) => execute_key_map_command(output_file_params, key_map_params),
        Options::Dump(DumpOptions {
            key_map_params,
            limit_params,
            command,
        }) => dump_scale(key_map_params, limit_params.limit, command),
        Options::JsonDump(JsonDumpOptions {
            key_map_params,
            command,
        }) => jdump_scale(key_map_params, command),
        Options::Diff(DiffOptions {
            limit_params,
            key_map_params,
            command,
        }) => diff_scale(key_map_params, limit_params.limit, command),
        Options::Mts(MtsOptions {
            key_map_params,
            command,
        }) => dump_mts(key_map_params, command),
    }
}

fn execute_scale_command(
    output_file_params: OutputFileParams,
    command: ScaleCommand,
) -> io::Result<()> {
    generate_output(output_file_params, create_scale(command).as_scl())
}

fn execute_key_map_command(
    output_file_params: OutputFileParams,
    key_map_params: KeyMapParams,
) -> io::Result<()> {
    generate_output(output_file_params, create_key_map(key_map_params).as_kbm())
}

fn dump_scale(key_map_params: KeyMapParams, limit: u16, command: ScaleCommand) -> io::Result<()> {
    let scale = create_scale(command);
    let key_map = create_key_map(key_map_params);
    let scale_with_key_map = scale.with_key_map(&key_map);

    let stdout = io::stdout();
    let mut printer = ScaleTablePrinter {
        write: &mut stdout.lock(),
        root_pitch: scale_with_key_map.pitch_of(0),
        limit,
    };

    printer.print_table_header()?;
    for scale_item in scale_iter(scale_with_key_map) {
        let approximation: Approximation<Note> = scale_item.pitch.find_in(ConcertPitch::default());

        printer.print_table_row(
            scale_item.midi_number,
            key_map.root_key.num_keys_before(scale_item.piano_key),
            scale_item.pitch,
            approximation.approx_value.midi_number(),
            format!("{:>9}", approximation.approx_value),
            approximation.deviation,
        )?;
    }
    Ok(())
}

fn jdump_scale(key_map_params: KeyMapParams, command: ScaleCommand) -> io::Result<()> {
    let scale = create_scale(command);
    let key_map = create_key_map(key_map_params);

    let mut dump_items = Vec::new();
    for scale_item in scale_iter(scale.with_key_map(&key_map)) {
        dump_items.push(DumpItemDto {
            key_midi_number: scale_item.piano_key.midi_number(),
            scale_degree: key_map.root_key.num_keys_before(scale_item.piano_key),
            pitch_in_hz: scale_item.pitch.as_hz(),
        });
    }

    let dump = DumpDto { items: dump_items };
    let dto = TuneDto::Dump(dump);

    writeln!(
        io::stdout().lock(),
        "{}",
        serde_json::to_string_pretty(&dto).unwrap()
    )
}

fn scale_iter<'a>(tuning: ScaleWithKeyMap<'a, 'a>) -> impl 'a + Iterator<Item = ScaleItem> {
    (0..128).map(move |midi_number| {
        let piano_key = PianoKey::from_midi_number(midi_number);
        ScaleItem {
            midi_number,
            piano_key,
            pitch: tuning.pitch_of(piano_key),
        }
    })
}

struct ScaleItem {
    midi_number: i32,
    piano_key: PianoKey,
    pitch: Pitch,
}

fn diff_scale(key_map_params: KeyMapParams, limit: u16, command: ScaleCommand) -> io::Result<()> {
    let input: TuneDto =
        serde_json::from_reader(io::stdin().lock()).expect("Input is not a JSON parseable by tune");

    let TuneDto::Dump(in_scale) = input;

    let scale = create_scale(command);
    let key_map = create_key_map(key_map_params);
    let scale_with_key_map = scale.with_key_map(&key_map);

    let stdout = io::stdout();
    let mut printer = ScaleTablePrinter {
        write: &mut stdout.lock(),
        root_pitch: scale_with_key_map.pitch_of(0),
        limit,
    };

    printer.print_table_header()?;
    for item in in_scale.items {
        let pitch = Pitch::from_hz(item.pitch_in_hz);

        let approximation: Approximation<PianoKey> = pitch.find_in(scale_with_key_map);
        let index = key_map.root_key.num_keys_before(approximation.approx_value);

        printer.print_table_row(
            item.key_midi_number,
            item.scale_degree,
            pitch,
            approximation.approx_value.midi_number(),
            format!("IDX {:>5}", index),
            approximation.deviation,
        )?;
    }
    Ok(())
}

struct ScaleTablePrinter<W> {
    write: W,
    root_pitch: Pitch,
    limit: u16,
}

impl<W: Write> ScaleTablePrinter<W> {
    fn print_table_header(&mut self) -> io::Result<()> {
        writeln!(
            self.write,
            "  {source:-^33} ‖ {pitch:-^14} ‖ {target:-^28}",
            source = "Source Scale",
            pitch = "Pitch",
            target = "Target Scale"
        )?;
        Ok(())
    }

    fn print_table_row(
        &mut self,
        source_midi: i32,
        source_index: i32,
        pitch: Pitch,
        target_midi: i32,
        target_index: String,
        deviation: Ratio,
    ) -> io::Result<()> {
        if source_index == 0 {
            write!(self.write, "> ")?;
        } else {
            write!(self.write, "  ")?;
        }

        let nearest_fraction =
            Ratio::between_pitches(self.root_pitch, pitch).nearest_fraction(self.limit);

        writeln!(
            self.write,
            "{source_midi:>3} | IDX {source_index:>4} | \
             {numer:>2}/{denom:<2} {fract_deviation:>+4.0}c {fract_octaves:>+3}o ‖ \
             {pitch:>11.3} Hz ‖ {target_midi:>4} | {target_index} | {deviation:>+8.3}¢",
            source_midi = source_midi,
            source_index = source_index,
            pitch = pitch.as_hz(),
            numer = nearest_fraction.numer,
            denom = nearest_fraction.denom,
            fract_deviation = nearest_fraction.deviation.as_cents(),
            fract_octaves = nearest_fraction.num_octaves,
            target_midi = target_midi,
            target_index = target_index,
            deviation = deviation.as_cents(),
        )
    }
}

fn dump_mts(key_map_params: KeyMapParams, command: ScaleCommand) -> io::Result<()> {
    let scale = create_scale(command);
    let key_map = create_key_map(key_map_params);

    let tuning_message =
        SingleNoteTuningChangeMessage::from_scale(&scale, &key_map, Default::default()).unwrap();

    for byte in tuning_message.sysex_bytes() {
        writeln!(io::stdout().lock(), "0x{:02x}", byte)?;
    }

    writeln!(
        io::stdout().lock(),
        "Number of retuned notes: {}",
        tuning_message.retuned_notes().len()
    )?;
    writeln!(
        io::stdout().lock(),
        "Number of out-of-range notes: {}",
        tuning_message.out_of_range_notes().len()
    )?;
    Ok(())
}

fn create_scale(command: ScaleCommand) -> Scale {
    match command {
        ScaleCommand::EqualTemperament { step_size } => {
            scale::create_equal_temperament_scale(step_size)
        }
        ScaleCommand::Rank2Temperament {
            generator,
            num_pos_generations,
            num_neg_generations,
            period,
        } => scale::create_rank2_temperament_scale(
            generator,
            num_pos_generations,
            num_neg_generations,
            period,
        ),
        ScaleCommand::HarmonicSeries {
            lowest_harmonic,
            number_of_notes,
            subharmonics,
        } => scale::create_harmonics_scale(
            u32::from(lowest_harmonic),
            u32::from(number_of_notes.unwrap_or(lowest_harmonic)),
            subharmonics,
        ),
        ScaleCommand::Custom { items, name } => {
            create_custom_scale(items, name.unwrap_or_else(|| "Custom scale".to_string()))
        }
    }
}

fn create_custom_scale(items: Vec<Ratio>, name: String) -> Scale {
    let mut scale = Scale::with_name(name);
    for item in items {
        scale.push_ratio(item);
    }
    scale.build()
}

fn create_key_map(key_map_params: KeyMapParams) -> KeyMap {
    KeyMap {
        ref_pitch: key_map_params.ref_pitch,
        root_key: key_map_params
            .root_note
            .map(i32::from)
            .map(PianoKey::from_midi_number)
            .unwrap_or_else(|| key_map_params.ref_pitch.key()),
    }
}

fn generate_output<D: Display>(output_file_params: OutputFileParams, content: D) -> io::Result<()> {
    if let Some(output_file) = output_file_params.output_file {
        File::create(output_file).and_then(|mut file| write!(file, "{}", content))
    } else {
        write!(io::stdout().lock(), "{}", content)
    }
}
