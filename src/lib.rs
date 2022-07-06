// Copyright (C) 2022 Andrew Archibald
//
// yspace2 is free software: you can redistribute it and/or modify it under the
// terms of the GNU Lesser General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option) any
// later version. You should have received a copy of the GNU Lesser General
// Public License along with yspace2. If not, see http://www.gnu.org/licenses/.

#![feature(
    box_syntax,
    concat_bytes,
    const_array_into_iter_constructors,
    const_convert,
    const_default_impls,
    const_for,
    const_intoiterator_identity,
    const_mut_refs,
    const_trait_impl,
    inline_const,
    never_type,
    trait_alias
)]

pub mod ws {
    pub mod bit_pack;
    pub mod inst;
    pub mod lex;
    pub mod parse;
    pub mod program;
    pub mod token;
    mod token_vec;

    #[cfg(test)]
    mod tests;
}
