// Copyright (C) 2022 Andrew Archibald
//
// yspace2 is free software: you can redistribute it and/or modify it under the
// terms of the GNU Lesser General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option) any
// later version. You should have received a copy of the GNU Lesser General
// Public License along with yspace2. If not, see http://www.gnu.org/licenses/.

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

use std::mem::size_of;

use bitvec::prelude::*;
use rug::{integer::Order, ops::NegAssign, Integer};
use static_assertions::assert_type_eq_all;

assert_type_eq_all!(BitSlice, BitSlice<usize, Lsb0>);
assert_type_eq_all!(BitVec, BitVec<usize, Lsb0>);
assert_type_eq_all!(BitBox, BitBox<usize, Lsb0>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sign {
    Pos,
    Neg,
    Empty,
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
        if len < size_of::<[usize; 4]>() * u8::BITS as usize {
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
