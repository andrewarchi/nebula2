// Copyright (C) 2022 Andrew Archibald
//
// yspace2 is free software: you can redistribute it and/or modify it under the
// terms of the GNU Lesser General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option) any
// later version. You should have received a copy of the GNU Lesser General
// Public License along with yspace2. If not, see http://www.gnu.org/licenses/.

#![feature(box_syntax)]

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use clap::{Args, Parser as CliParser, Subcommand};
use yspace2::ws::{
    bit_pack::BitLexer,
    inst::{Feature, Features, Inst},
    lex::{ByteLexer, Lexer, Utf8Lexer},
    parse::Parser,
    token::Mapping,
};

#[derive(Debug, CliParser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Detect the spec version (0.2 or 0.3) for a program and any non-standard
    /// instructions
    DetectFeatures(ProgramOptions),
}

#[derive(Debug, Args)]
struct ProgramOptions {
    /// Path to Whitespace program
    #[clap(required = true, value_parser)]
    filename: PathBuf,
    /// Disable UTF-8 validation
    #[clap(long, value_parser, default_value_t = false)]
    ascii: bool,
}

fn main() {
    let args = Cli::parse();
    match args.command {
        Command::DetectFeatures(program) => detect_features(program),
    }
}

fn detect_features(program: ProgramOptions) {
    let src = fs::read(&program.filename).unwrap();
    let ext = program.filename.extension().map(OsStr::to_str).flatten();
    let lex: Box<dyn Lexer> = match ext {
        Some("wsx") => box BitLexer::new(&src),
        _ if program.ascii => box ByteLexer::new(&src, Mapping::<u8>::default()),
        _ => box Utf8Lexer::new(&src, Mapping::<char>::default()),
    };
    let parser = Parser::new(lex, Features::all()).unwrap();

    let mut features = Features::empty();
    for inst in parser {
        if let Inst::Error(err) = inst {
            println!("error: {:?}", err);
        } else if let Some(feature) = inst.opcode().feature() {
            features.insert(feature);
        }
    }
    if !features.contains(Feature::Wspace0_3) {
        println!("wspace 0.2");
    }
    for feature in features {
        println!("{}", feature);
    }
}
