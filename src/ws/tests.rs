// Copyright (C) 2022 Andrew Archibald
//
// yspace2 is free software: you can redistribute it and/or modify it under the
// terms of the GNU Lesser General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option) any
// later version. You should have received a copy of the GNU Lesser General
// Public License along with yspace2. If not, see http://www.gnu.org/licenses/.

use bitvec::prelude::*;

use crate::ws::inst::{Inst, Int, Sign, Uint};
use crate::ws::token::Token::{self, *};

pub const TUTORIAL_STL: &str = r"
S S S T L                    push 1
L S S S T S S S S T T L  label_C:
S L S                        dup
T L S T                      printi
S S S T S T S L              push 10
T L S S                      printc
S S S T L                    push 1
T S S S                      add
S L S                        dup
S S S T S T T L              push 11
T S S T                      sub
L T S S T S S S T S T L      jz label_E
L S L S T S S S S T T L      jmp label_C
L S S S T S S S T S T L  label_E:
S L L                        drop
L L L                        end
";

pub const TUTORIAL_TOKENS: &[Token] = &[
    S, S, S, T, L, L, S, S, S, T, S, S, S, S, T, T, L, S, L, S, T, L, S, T, S, S, S, T, S, T, S, L,
    T, L, S, S, S, S, S, T, L, T, S, S, S, S, L, S, S, S, S, T, S, T, T, L, T, S, S, T, L, T, S, S,
    T, S, S, S, T, S, T, L, L, S, L, S, T, S, S, S, S, T, T, L, L, S, S, S, T, S, S, S, T, S, T, L,
    S, L, L, L, L, L,
];

pub const TUTORIAL_BITS: &[u8] = &[
    0b00010111, 0b10001000, 0b00101011, 0b01101011, 0b01000010, 0b01001110, 0b11000001, 0b01110000,
    0b01100001, 0b00101011, 0b10001011, 0b10001000, 0b01001011, 0b11011010, 0b00001010, 0b11110001,
    0b00001001, 0b01101111, 0b11111100,
];

pub fn tutorial_insts() -> Vec<Inst> {
    let label_c = Uint {
        bits: bitvec![0, 1, 0, 0, 0, 0, 1, 1],
    };
    let label_e = Uint {
        bits: bitvec![0, 1, 0, 0, 0, 1, 0, 1],
    };
    vec![
        Inst::Push(Int {
            sign: Sign::Pos,
            bits: bitvec![1],
        }),
        Inst::Label(label_c.clone()),
        Inst::Dup,
        Inst::Printi,
        Inst::Push(Int {
            sign: Sign::Pos,
            bits: bitvec![1, 0, 1, 0],
        }),
        Inst::Printc,
        Inst::Push(Int {
            sign: Sign::Pos,
            bits: bitvec![1],
        }),
        Inst::Add,
        Inst::Dup,
        Inst::Push(Int {
            sign: Sign::Pos,
            bits: bitvec![1, 0, 1, 1],
        }),
        Inst::Sub,
        Inst::Jz(label_e.clone()),
        Inst::Jmp(label_c),
        Inst::Label(label_e),
        Inst::Drop,
        Inst::End,
    ]
}