use std::{fs, io::Write, path::PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};
use embedded_perfmon_analyzer::{Capture, deserialize_events};

/// Analyzer of trace data
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// What to do
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug, Clone)]
enum Command {
    ParseToJson(ParseArgs),
    ParseToPerfetto(ParseArgs),
    Schema {
        /// The path where the schema json is saved.
        /// If not specified, the schema is outputted to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(clap::Args, Debug, Clone)]
struct ParseArgs {
    #[command(flatten)]
    source: Source,
    /// The path where the output json is saved.
    /// If not specified, the json is outputted to stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(clap::Args, Debug, Clone)]
#[group(required = true, multiple = false)]
struct Source {
    #[arg(long)]
    file: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::ParseToJson(args) => parse_to_json(args),
        Command::ParseToPerfetto(args) => parse_to_perfetto(args),
        Command::Schema { output } => schema(output),
    }
}

fn schema(output: Option<PathBuf>) -> anyhow::Result<()> {
    let schema = schemars::SchemaGenerator::default().root_schema_for::<Capture>();

    if let Some(output_path) = output {
        let mut file = fs::File::create(&output_path).context(format!(
            "creating output path at: {}",
            output_path.display()
        ))?;
        serde_json::to_writer_pretty(&mut file, &schema)
            .context("serializing schema to json and writing to file")?;
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&schema).context("serializing schema to json")?
        );
    }

    Ok(())
}

fn parse_to_json(args: ParseArgs) -> anyhow::Result<()> {
    let mut bytes = match args.source.file {
        Some(path) => collect_from_file(path)?,
        _ => unreachable!(),
    };

    let events = deserialize_events(&mut bytes)?;

    let capture = Capture::parse_events(&events);

    if let Some(output_path) = args.output {
        let mut file = fs::File::create(&output_path).context(format!(
            "creating output path at: {}",
            output_path.display()
        ))?;
        serde_json::to_writer_pretty(&mut file, &capture)
            .context("serializing traces to json and writing to file")?;
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&capture).context("serializing traces to json")?
        );
    }

    Ok(())
}

fn parse_to_perfetto(args: ParseArgs) -> anyhow::Result<()> {
    let mut bytes = match args.source.file {
        Some(path) => collect_from_file(path)?,
        _ => unreachable!(),
    };

    let events = deserialize_events(&mut bytes)?;

    let trace = embedded_perfmon_analyzer::perfetto::to_perfetto_trace(&events);

    if let Some(output_path) = args.output {
        let mut file = fs::File::create(&output_path).context(format!(
            "creating output path at: {}",
            output_path.display()
        ))?;
        file.write_all(&perfetto_protos::serialize_trace(trace))
            .context("writing trace to file")?;
    } else {
        println!("{trace:?}",);
    }

    Ok(())
}

fn collect_from_file(path: PathBuf) -> anyhow::Result<Vec<u8>> {
    fs::read(path).context("reading input file")
}
