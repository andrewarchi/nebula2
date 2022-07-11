// Copyright (C) 2022 Andrew Archibald
//
// Nebula 2 is free software: you can redistribute it and/or modify it under the
// terms of the GNU Lesser General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option) any
// later version. You should have received a copy of the GNU Lesser General
// Public License along with Nebula 2. If not, see http://www.gnu.org/licenses/.

//! Defines arbitrary-precision integer and bit vector types that work together.
//!
//! To convert between Rug and `bitvec` types, the bit order, endianness, and
//! limb size need to correspond:
//!
//! In the GMP `mpz_import` function, which Rug calls in `Integer::from_digits`,
//! it copies the input data to its internal format, which appears to be `Lsf`.
//! If the input order is `Lsf*` and the endianness matches the host, the data
//! is simply copied. If the endianness does not match the host, it swaps the
//! bytes. If the input order is `Msf*`, the bits are reversed.
//!
//! `bitvec` strongly recommends using `Lsb0` as the `BitOrder`, even if it
//! doesn't match the host endianness, because it provides the best codegen for
//! bit manipulation. Since there is no equivalent to `Lsf` in `bitvec` and
//! big-endian systems are rare, `LsfLe`/`Lsb0` is the best option.
//!
//! Whitespace integers are big endian, but are parsed and pushed to a `BitVec`
//! in little-endian order, so the slice of bits needs to be reversed (i.e., not
//! reversing or swapping words) before converting to an `Integer`.
//!
//! GMP uses a machine word as the limb size and `bitvec` uses `usize` as the
//! default `BitStore`.
//!
//! | Rug   | bitvec    | Bit order                   | Endianness      |
//! | ----- | --------- | --------------------------- | --------------- |
//! | Lsf   |           | least-significant bit first | host endianness |
//! | LsfLe | Lsb0      | least-significant bit first | little-endian   |
//! | LsfBe |           | least-significant bit first | big-endian      |
//! | Msf   |           | most-significant bit first  | host endianness |
//! | MsfLe |           | most-significant bit first  | little-endian   |
//! | MsfBe | Msb0      | most-significant bit first  | big-endian      |
//! |       | LocalBits | alias to Lsb0 or Msb0       | host endianness |

use std::fmt::{self, Display, Formatter};
use std::intrinsics::unlikely;
use std::ops::{Deref, DerefMut};

use bitvec::prelude::*;
use compact_str::CompactString;
use gmp_mpfr_sys::gmp;
use rug::{integer::Order, ops::NegAssign, Integer};
use static_assertions::assert_type_eq_all;

assert_type_eq_all!(BitSlice, BitSlice<usize, Lsb0>);
assert_type_eq_all!(BitVec, BitVec<usize, Lsb0>);
assert_type_eq_all!(BitBox, BitBox<usize, Lsb0>);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IntLiteral {
    /// Bit representation with the sign in the first bit (if nonempty) and
    /// possible leading zeros.
    bits: BitVec,
    /// String representation from Whitespace assembly source.
    string: Option<CompactString>,
    /// Numeric representation.
    int: Integer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sign {
    Pos,
    Neg,
    Empty,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ParseError {
    InvalidRadix,
    InvalidDigit { ch: char, offset: usize },
    IllegalUnderscore { offset: usize },
    NoDigits,
}

impl IntLiteral {
    /// Parses an integer with the given radix. A radix in 2..=36 uses the
    /// case-insensitive alphabet 0-9A-Z, so upper- and lowercase letters are
    /// equivalent. A radix in 37..=62 uses the case-sensitive alphabet
    /// 0-9A-Za-z.
    pub fn parse_radix(s: CompactString, radix: u32) -> Result<Self, ParseError> {
        let b = s.as_bytes();
        let (sign, offset) = Self::parse_sign(b);
        if offset == b.len() {
            return Err(ParseError::NoDigits);
        }
        Self::parse_digits(s, offset, sign, radix)
    }

    /// Parses an Erlang-like integer literal of the form `base#value`, where
    /// `base` is in 2..=62, or unprefixed `value` (for base 10). Single
    /// underscores may separate digits.
    ///
    /// Additionally, unlike Erlang:
    /// - bases 37..=62 are also allowed, which use the case-sensitive alphabet
    ///   0-9A-Za-z
    /// - letter aliases may be used for `base`: `b`/`B` for binary, `o`/`O` for
    ///   octal, and `x`/`X` for hexadecimal
    /// - and `value` may be empty, to allow for expressing all forms of
    ///   Whitespace bit patterns
    ///
    /// Specifically, it has the grammar
    /// `/[+-]?((\d{1,2}|[bBoOxX])#)?[0-9A-Za-z][0-9A-Za-z_]*/`, with the base
    /// and digits checked to be in range.
    pub fn parse_erlang_style(s: CompactString) -> Result<Self, ParseError> {
        let b = s.as_bytes();
        let (sign, offset) = Self::parse_sign(b);
        let (radix, offset) = match b[offset..] {
            [r, b'#', ..] => (
                match r {
                    b'b' | b'B' => 2,
                    b'o' | b'O' => 8,
                    b'x' | b'X' => 16,
                    b'2'..=b'9' => (r - b'0') as u32,
                    _ => return Err(ParseError::InvalidRadix),
                },
                offset + 2,
            ),
            [r0, r1, b'#', ..] => (
                match (r0, r1) {
                    (b'1'..=b'6', b'0'..=b'9') => ((r0 - b'0') * 10 + r1 - b'0') as u32,
                    _ => return Err(ParseError::InvalidRadix),
                },
                offset + 3,
            ),
            // Allow the digits to be omitted only with an explicit radix.
            [] => return Err(ParseError::NoDigits),
            _ => (10, offset),
        };
        match Self::parse_digits(s, offset, sign, radix) {
            Err(ParseError::InvalidDigit { ch, .. }) if ch == '#' => Err(ParseError::InvalidRadix),
            result => result,
        }
    }

    /// Parses a C-like integer literal with an optional prefix that denotes the
    /// base: `0b`/`0B` for binary, `0`/`0o`/`0O` for octal, `0x`/`0X` for
    /// hexadecimal, or decimal otherwise. Single underscores may separate
    /// digits.
    ///
    /// Specifically, it has the grammar
    /// `/[+-]?(0[bBoOxX]?)?[0-9A-Za-z][0-9A-Za-z_]*/`, with the base and digits
    /// checked to be in range.
    pub fn parse_c_style(s: CompactString) -> Result<Self, ParseError> {
        let b = s.as_bytes();
        let (sign, offset) = Self::parse_sign(b);
        let (radix, offset) = match b[offset..] {
            [b'0', b'b' | b'B', ..] => (2, offset + 2),
            [b'0', b'o' | b'O', ..] => (8, offset + 2),
            [b'0', b'x' | b'X', ..] => (16, offset + 2),
            [b'0', _, ..] => (8, offset + 1),
            _ => (10, offset),
        };
        if offset == b.len() {
            return Err(ParseError::NoDigits);
        }
        Self::parse_digits(s, offset, sign, radix)
    }

    #[inline]
    fn parse_sign(b: &[u8]) -> (Sign, usize) {
        match b.first() {
            Some(b'+') => (Sign::Pos, 1),
            Some(b'-') => (Sign::Neg, 1),
            _ => (Sign::Empty, 0),
        }
    }

    fn parse_digits(
        s: CompactString,
        offset: usize,
        sign: Sign,
        radix: u32,
    ) -> Result<Self, ParseError> {
        let table: &[u8; 256] = match radix {
            2..=36 => RADIX_DIGIT_VALUES.split_array_ref().0,
            37..=62 => RADIX_DIGIT_VALUES[208..].try_into().unwrap(),
            _ => return Err(ParseError::InvalidRadix),
        };

        // Skip leading zeros
        let mut b = &s.as_bytes()[offset..];
        let mut leading_zeros = 0usize;
        while let Some((ch, b1)) = b.split_first() {
            match ch {
                b'0' => leading_zeros += 1,
                b'_' if leading_zeros != 0 => {} // TODO: Fix
                _ => break,
            }
            b = b1;
        }
        // Only use leading zeros for power-of-two radices and zero
        let leading_zeros = if leading_zeros != 0 && radix.is_power_of_two() {
            // TODO: Handle leading zeros in the binary representation of the
            // first char
            leading_zeros * radix.log2() as usize
        } else if leading_zeros != 0 && b.is_empty() {
            1
        } else {
            0
        };
        if b.is_empty() {
            return Ok(IntLiteral {
                bits: Self::signed_bitvec_from_zero(sign, leading_zeros),
                string: Some(s),
                int: Integer::new(),
            });
        }

        let mut digits = Vec::with_capacity(b.len());
        for (i, &ch) in b.iter().enumerate() {
            let digit = table[ch as usize];
            if unlikely(digit as u32 >= radix) {
                if ch == b'_' {
                    if i == 0 || i == b.len() - 1 || b[i - 1] == b'_' {
                        return Err(ParseError::IllegalUnderscore {
                            offset: s.len() - b.len() + i,
                        });
                    }
                    continue;
                }
                // The invalid digit may be non-ASCII; decode it
                return Err(ParseError::InvalidDigit {
                    ch: bstr::decode_utf8(&b[i..]).0.unwrap_or('\u{FFFD}'),
                    offset: s.len() - b.len() + i,
                });
            }
            digits.push(digit);
        }

        let int = Self::integer_from_digits(digits, sign, radix);
        let bits = Self::signed_bitvec_from_integer(&int, sign, leading_zeros);
        Ok(IntLiteral { bits, int, string: Some(s) })
    }

    /// Constructs an Integer from a Vec of digits, where each digit is in the
    /// range 0..radix, i.e., not ASCII characters.
    ///
    /// Adapted from Rug internal functions:
    /// `<rug::integer::ParseIncomplete as rug::Assign>::assign`
    /// and `rug::ext::xmpz::realloc_for_mpn_set_str`.
    ///
    /// Compensates for Rug missing a higher-level API for using `mpn_set_str`:
    /// https://gitlab.com/tspiteri/rug/-/issues/41
    fn integer_from_digits(digits: Vec<u8>, sign: Sign, radix: u32) -> Integer {
        let mut int = Integer::new();
        let raw = int.as_raw_mut();

        // Add 1 to make the floored integer log be ceiling
        let bits = (radix.log2() as usize + 1) * digits.len();
        // Use integer ceiling division
        let limb_bits = gmp::LIMB_BITS as usize;
        let limbs = (bits + limb_bits - 1) / limb_bits;
        unsafe {
            // Add 1, because `mpn_set_str` requires an extra limb
            gmp::_mpz_realloc(raw, limbs as gmp::size_t + 1);

            let size = gmp::mpn_set_str(
                (*raw).d.as_ptr(),
                digits.as_ptr(),
                digits.len(),
                radix as i32,
            );
            (*raw).size = if sign == Sign::Neg { -size } else { size } as i32;
        }
        int
    }

    fn unsigned_bitvec_from_integer(int: &Integer) -> BitVec {
        let mut bits = BitVec::<usize, Lsb0>::from_vec(int.to_digits(Order::LsfLe));
        bits.truncate(bits.last_one().map_or(0, |i| i + 1));
        bits.reverse();
        bits
    }

    fn signed_bitvec_from_integer(int: &Integer, sign: Sign, leading_zeros: usize) -> BitVec {
        let mut bits = Self::unsigned_bitvec_from_integer(int);
        let len = bits.len();
        // Newly-reserved bits are guaranteed to be allocated to zero
        bits.reserve(leading_zeros + 1);
        unsafe { bits.set_len(len + leading_zeros + 1) };
        // Panics when shifting by the length
        if len != 0 {
            bits.shift_right(leading_zeros + 1);
        }
        if sign == Sign::Neg {
            bits.set(0, true);
        }
        bits
    }

    fn signed_bitvec_from_zero(sign: Sign, n_zeros: usize) -> BitVec {
        if n_zeros == 0 && sign == Sign::Empty {
            return BitVec::new();
        }
        let mut bits = BitVec::repeat(false, n_zeros + 1);
        if sign == Sign::Neg {
            bits.set(0, true);
        }
        bits
    }

    #[inline]
    pub fn sign(&self) -> Sign {
        self.bits.as_bitslice().sign()
    }
}

const X: u8 = 0xff;
/// Table indexed by ASCII character, to get its numeric value. The first part
/// of the table (0..256) is for radices 2..=36 that use the case-insensitive
/// alphabet 0-9A-Z, so upper- and lowercase letters map to the same digit. The
/// second part (208..=464) is for radices 37..=62 that use the case-sensitive
/// alphabet 0-9A-Za-z.
///
/// Copied from __gmp_digit_value_tab in gmp-6.2.1/mp_dv_tab.c
#[rustfmt::skip]
static RADIX_DIGIT_VALUES: [u8; 464] = [
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, X, X, X, X, X, X,
    X,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,
    25,26,27,28,29,30,31,32,33,34,35,X, X, X, X, X,
    X,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,
    25,26,27,28,29,30,31,32,33,34,35,X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, X, X, X, X, X, X,
    X,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,
    25,26,27,28,29,30,31,32,33,34,35,X, X, X, X, X,
    X,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,
    51,52,53,54,55,56,57,58,59,60,61,X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
];

impl From<BitVec> for IntLiteral {
    #[inline]
    fn from(bits: BitVec) -> Self {
        let int = bits.to_int();
        IntLiteral { bits, string: None, int }
    }
}

impl Deref for IntLiteral {
    type Target = Integer;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.int
    }
}

impl DerefMut for IntLiteral {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.int
    }
}

impl Display for IntLiteral {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(ref s) = self.string {
            f.write_str(s.as_str())
        } else {
            if self.bits.get(1).as_deref() == Some(&true) || self.bits.len() == 2 {
                write!(f, "{}", self.int)
            } else {
                // Write numbers with leading zeros in base 2
                let sign = if self.bits.get(0).as_deref() == Some(&true) {
                    "-"
                } else if self.bits.len() == 1 {
                    // Sign-only numbers need an explicit positive sign
                    "+"
                } else {
                    ""
                };
                let bin = self.bits[1..]
                    .iter()
                    .map(|b| if *b { '1' } else { '0' })
                    .collect::<String>();
                write!(f, "{sign}b#{bin}")
            }
        }
    }
}

pub trait ToInteger {
    fn to_int(&self) -> Integer;
    fn to_uint(&self) -> Integer;
    fn to_uint_unambiguous(&self) -> Option<Integer>;
    fn sign(&self) -> Sign;
}

impl ToInteger for BitSlice {
    fn to_int(&self) -> Integer {
        match self.split_first() {
            None => Integer::ZERO,
            Some((sign, bits)) => {
                let mut int = bits.to_uint();
                if *sign == true {
                    int.neg_assign();
                }
                int
            }
        }
    }

    fn to_uint(&self) -> Integer {
        let len = self.len();
        if len < usize::BITS as usize * 4 {
            let mut arr = BitArray::<_, Lsb0>::new([0usize; 4]);
            let slice = &mut arr[..len];
            slice.copy_from_bitslice(self);
            slice.reverse();
            Integer::from_digits(arr.as_raw_slice(), Order::LsfLe)
        } else {
            let mut boxed = BitBox::<usize, Lsb0>::from(self);
            boxed.force_align();
            boxed.fill_uninitialized(false);
            boxed.reverse();
            Integer::from_digits(boxed.as_raw_slice(), Order::LsfLe)
        }
    }

    #[inline]
    fn to_uint_unambiguous(&self) -> Option<Integer> {
        if self.first().as_deref() == Some(&true) {
            Some(self.to_uint())
        } else {
            None
        }
    }

    #[inline]
    fn sign(&self) -> Sign {
        match self.first().as_deref() {
            Some(true) => Sign::Neg,
            Some(false) => Sign::Pos,
            None => Sign::Empty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ParseTest {
        syntax: SyntaxStyle,
        string: &'static str,
        result: Result<IntLiteral, ParseError>,
    }
    #[derive(Debug)]
    enum SyntaxStyle {
        Erlang,
        C,
    }

    macro_rules! parse_test_ok(
        ($syntax:ident, $s:literal, $int:literal, [$($bit:literal)*]) => {
            ParseTest {
                syntax: SyntaxStyle::$syntax,
                string: $s,
                result: Ok(IntLiteral {
                    bits: bitvec![$($bit),*],
                    string: Some($s.into()),
                    int: Integer::parse($int).unwrap().into(),
                }),
            }
        }
    );
    macro_rules! parse_test_err(
        ($syntax:ident, $s:literal, $err:expr) => {
            ParseTest {
                syntax: SyntaxStyle::$syntax,
                string: $s.into(),
                result: Err($err),
            }
        }
    );

    #[test]
    fn parse() {
        use ParseError::*;
        for test in [
            parse_test_ok!(Erlang, "0", "0", [0 0]),
            parse_test_ok!(Erlang, "00", "00", [0 0]),
            parse_test_ok!(Erlang, "000", "000", [0 0]),
            parse_test_ok!(Erlang, "b#", "0", []),
            parse_test_ok!(Erlang, "+b#", "0", [0]),
            parse_test_ok!(Erlang, "-b#", "0", [1]),
            parse_test_ok!(Erlang, "b#0", "0", [0 0]),
            parse_test_ok!(Erlang, "+b#0", "0", [0 0]),
            parse_test_ok!(Erlang, "-b#0", "0", [1 0]),
            parse_test_ok!(Erlang, "42", "42", [0 1 0 1 0 1 0]),
            parse_test_ok!(Erlang, "16#123", "291", [0 0 0 0 1 0 0 1 0 0 0 1 1]),
            parse_test_ok!(Erlang, "16#dead_BEEF", "3735928559", [0 1 1 0 1 1 1 1 0 1 0 1 0 1 1 0 1 1 0 1 1 1 1 1 0 1 1 1 0 1 1 1 1]),
            parse_test_ok!(Erlang, "-60#100", "-3600", [1 1 1 1 0 0 0 0 1 0 0 0 0]),
            parse_test_err!(Erlang, "3#_0", IllegalUnderscore { offset: 2 }),
            parse_test_err!(Erlang, "21#0_", IllegalUnderscore { offset: 4 }),
            parse_test_err!(Erlang, "42#9__2", IllegalUnderscore { offset: 5 }),
            parse_test_err!(Erlang, "b", InvalidDigit { ch: 'b', offset: 0 }),
            parse_test_err!(Erlang, "B", InvalidDigit { ch: 'B', offset: 0 }),
            parse_test_err!(Erlang, "o", InvalidDigit { ch: 'o', offset: 0 }),
            parse_test_err!(Erlang, "O", InvalidDigit { ch: 'O', offset: 0 }),
            parse_test_err!(Erlang, "x", InvalidDigit { ch: 'x', offset: 0 }),
            parse_test_err!(Erlang, "X", InvalidDigit { ch: 'X', offset: 0 }),
            parse_test_err!(Erlang, "#", InvalidRadix),
            parse_test_err!(Erlang, "a#", InvalidRadix),
            parse_test_err!(Erlang, "ab#", InvalidRadix),
            parse_test_err!(Erlang, "abc#", InvalidDigit { ch: 'a', offset: 0 }),
            parse_test_err!(Erlang, "0#", InvalidRadix),
            parse_test_err!(Erlang, "1#", InvalidRadix),
            parse_test_err!(Erlang, "63#", InvalidRadix),
            parse_test_ok!(C, "0", "0", [0 0]),
            parse_test_ok!(C, "00", "00", [0 0 0 0]),
            parse_test_ok!(C, "000", "000", [0 0 0 0 0 0 0]),
            parse_test_ok!(C, "0755", "493", [0 1 1 1 1 0 1 1 0 1]),
            parse_test_err!(C, "0b", NoDigits),
            parse_test_err!(C, "0x", NoDigits),
            parse_test_err!(C, "0a", InvalidDigit { ch: 'a', offset: 1 }),
        ] {
            let result = match test.syntax {
                SyntaxStyle::Erlang => IntLiteral::parse_erlang_style(test.string.into()),
                SyntaxStyle::C => IntLiteral::parse_c_style(test.string.into()),
            };
            assert_eq!(
                test.result, result,
                "parse {} with {:?} syntax",
                test.string, test.syntax,
            );
        }
    }
}
