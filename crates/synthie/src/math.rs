//! Transcendental math wrappers.
//!
//! Under the `std` feature these delegate to the native `f32` methods (which in
//! turn call the platform libm or use hardware instructions).  Without `std` they
//! call into the `libm` crate, which is `no_std`-compatible and ships its own
//! portable implementations.
//!
//! For `no_std` builds, enable the `libm` feature alongside disabling `std`:
//! `--no-default-features --features libm`

#[cfg(all(not(feature = "std"), not(feature = "libm")))]
compile_error!(
    "Either the `std` or `libm` feature must be enabled. \
     For no_std targets, use: --no-default-features --features libm"
);

#[cfg(feature = "std")]
#[allow(dead_code)]
mod imp {
    #[inline]
    pub fn sinf(x: f32) -> f32 {
        x.sin()
    }
    #[inline]
    pub fn cosf(x: f32) -> f32 {
        x.cos()
    }
    #[inline]
    pub fn tanf(x: f32) -> f32 {
        x.tan()
    }
    #[inline]
    pub fn expf(x: f32) -> f32 {
        x.exp()
    }
    #[inline]
    pub fn powf(base: f32, exp: f32) -> f32 {
        base.powf(exp)
    }
    #[inline]
    pub fn sqrtf(x: f32) -> f32 {
        x.sqrt()
    }
    #[inline]
    pub fn sin_cos(x: f32) -> (f32, f32) {
        x.sin_cos()
    }
}

#[cfg(not(feature = "std"))]
#[allow(dead_code)]
mod imp {
    #[inline]
    pub fn sinf(x: f32) -> f32 {
        libm::sinf(x)
    }
    #[inline]
    pub fn cosf(x: f32) -> f32 {
        libm::cosf(x)
    }
    #[inline]
    pub fn tanf(x: f32) -> f32 {
        libm::tanf(x)
    }
    #[inline]
    pub fn expf(x: f32) -> f32 {
        libm::expf(x)
    }
    #[inline]
    pub fn powf(base: f32, exp: f32) -> f32 {
        libm::powf(base, exp)
    }
    #[inline]
    pub fn sqrtf(x: f32) -> f32 {
        // f32::sqrt() is a core compiler intrinsic — available in no_std.
        x.sqrt()
    }
    #[inline]
    pub fn sin_cos(x: f32) -> (f32, f32) {
        (libm::sinf(x), libm::cosf(x))
    }
}

pub use imp::*;
