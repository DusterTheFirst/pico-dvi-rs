#![no_std]

// https://github.com/rust-lang/rust/issues/115585 polyfill until stabilized
// Implementation taken from stdlib (https://github.com/rust-lang/rust/pull/115416/files#diff-7fdf8ef3b0e02b28e3caa4cc144046f9510df7c8a1f524124f4921601a3d7456)
macro_rules! cfg_match {
    (
        $(cfg($initial_meta:meta) => { $($initial_tokens:item)* })+
        _ => { $($extra_tokens:item)* }
    ) => {
        cfg_match! {
            @__items ();
            $((($initial_meta) ($($initial_tokens)*)),)+
            (() ($($extra_tokens)*)),
        }
    };
    (
        $(cfg($extra_meta:meta) => { $($extra_tokens:item)* })*
    ) => {
        cfg_match! {
            @__items ();
            $((($extra_meta) ($($extra_tokens)*)),)*
        }
    };
    (@__items ($($_:meta,)*);) => {};
    (
        @__items ($($no:meta,)*);
        (($($yes:meta)?) ($($tokens:item)*)),
        $($rest:tt,)*
    ) => {
        #[cfg(all(
            $($yes,)?
            not(any($($no),*))
        ))]
        cfg_match! { @__identity $($tokens)* }
        cfg_match! {
            @__items ($($no,)* $($yes,)?);
            $($rest,)*
        }
    };
    (@__identity $($tokens:item)*) => {
        $($tokens)*
    };
}

cfg_match! {
    cfg(feature = "rp-pico") => {
        pub use rp_pico::*;
    }
    cfg(feature = "adafruit-feather-rp2040") => {
        pub use adafruit_feather_rp2040::*;
    }
    _ => {
        compile_error!("One of the board support crate features must be enabled (see rp2040-bsp's Cargo.toml)");
    }
}
