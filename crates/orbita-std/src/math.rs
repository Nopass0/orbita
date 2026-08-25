#![allow(clippy::excessive_precision)]

/// Mathematical constants and `no_std` helpers re-exported by Orbita std.
///
/// The implementation uses `libm` so the API remains available in kernel and
/// freestanding targets without the host Rust standard library.
pub mod consts {
    pub const PI: f64 = core::f64::consts::PI;
    pub const TAU: f64 = core::f64::consts::TAU;
    pub const E: f64 = core::f64::consts::E;
    pub const FRAC_PI_2: f64 = core::f64::consts::FRAC_PI_2;
    pub const FRAC_PI_3: f64 = core::f64::consts::FRAC_PI_3;
    pub const FRAC_PI_4: f64 = core::f64::consts::FRAC_PI_4;
    pub const FRAC_PI_6: f64 = core::f64::consts::FRAC_PI_6;
    pub const FRAC_PI_8: f64 = core::f64::consts::FRAC_PI_8;
    pub const FRAC_1_PI: f64 = core::f64::consts::FRAC_1_PI;
    pub const FRAC_2_PI: f64 = core::f64::consts::FRAC_2_PI;
    pub const FRAC_2_SQRT_PI: f64 = core::f64::consts::FRAC_2_SQRT_PI;
    pub const SQRT_2: f64 = core::f64::consts::SQRT_2;
    pub const FRAC_1_SQRT_2: f64 = core::f64::consts::FRAC_1_SQRT_2;
    pub const LN_2: f64 = core::f64::consts::LN_2;
    pub const LN_10: f64 = core::f64::consts::LN_10;
    pub const LOG2_E: f64 = core::f64::consts::LOG2_E;
    pub const LOG10_E: f64 = core::f64::consts::LOG10_E;
}

#[inline]
pub fn abs(value: f64) -> f64 { libm::fabs(value) }
#[inline]
pub fn copysign(value: f64, sign: f64) -> f64 { libm::copysign(value, sign) }
#[inline]
pub fn floor(value: f64) -> f64 { libm::floor(value) }
#[inline]
pub fn ceil(value: f64) -> f64 { libm::ceil(value) }
#[inline]
pub fn round(value: f64) -> f64 { libm::round(value) }
#[inline]
pub fn trunc(value: f64) -> f64 { libm::trunc(value) }
#[inline]
pub fn fract(value: f64) -> f64 { value - trunc(value) }
#[inline]
pub fn recip(value: f64) -> f64 { 1.0 / value }
#[inline]
pub fn sqrt(value: f64) -> f64 { libm::sqrt(value) }
#[inline]
pub fn cbrt(value: f64) -> f64 { libm::cbrt(value) }
#[inline]
pub fn powf(value: f64, exponent: f64) -> f64 { libm::pow(value, exponent) }
#[inline]
pub fn powi(value: f64, exponent: i32) -> f64 { libm::pow(value, exponent as f64) }
#[inline]
pub fn hypot(x: f64, y: f64) -> f64 { libm::hypot(x, y) }
#[inline]
pub fn exp(value: f64) -> f64 { libm::exp(value) }
#[inline]
pub fn exp2(value: f64) -> f64 { libm::exp2(value) }
#[inline]
pub fn expm1(value: f64) -> f64 { libm::expm1(value) }
#[inline]
pub fn ln(value: f64) -> f64 { libm::log(value) }
#[inline]
pub fn log(base: f64, value: f64) -> f64 { ln(value) / ln(base) }
#[inline]
pub fn log2(value: f64) -> f64 { libm::log2(value) }
#[inline]
pub fn log10(value: f64) -> f64 { libm::log10(value) }
#[inline]
pub fn ln_1p(value: f64) -> f64 { libm::log1p(value) }
#[inline]
pub fn sin(value: f64) -> f64 { libm::sin(value) }
#[inline]
pub fn cos(value: f64) -> f64 { libm::cos(value) }
#[inline]
pub fn tan(value: f64) -> f64 { libm::tan(value) }
#[inline]
pub fn asin(value: f64) -> f64 { libm::asin(value) }
#[inline]
pub fn acos(value: f64) -> f64 { libm::acos(value) }
#[inline]
pub fn atan(value: f64) -> f64 { libm::atan(value) }
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 { libm::atan2(y, x) }
#[inline]
pub fn sinh(value: f64) -> f64 { libm::sinh(value) }
#[inline]
pub fn cosh(value: f64) -> f64 { libm::cosh(value) }
#[inline]
pub fn tanh(value: f64) -> f64 { libm::tanh(value) }
#[inline]
pub fn asinh(value: f64) -> f64 { libm::asinh(value) }
#[inline]
pub fn acosh(value: f64) -> f64 { libm::acosh(value) }
#[inline]
pub fn atanh(value: f64) -> f64 { libm::atanh(value) }
#[inline]
pub fn min(a: f64, b: f64) -> f64 { a.min(b) }
#[inline]
pub fn max(a: f64, b: f64) -> f64 { a.max(b) }
#[inline]
pub fn clamp(value: f64, min: f64, max: f64) -> f64 { value.clamp(min, max) }
#[inline]
pub fn signum(value: f64) -> f64 { value.signum() }
#[inline]
pub fn is_nan(value: f64) -> bool { value.is_nan() }
#[inline]
pub fn is_finite(value: f64) -> bool { value.is_finite() }
#[inline]
pub fn is_infinite(value: f64) -> bool { value.is_infinite() }
#[inline]
pub fn to_radians(value: f64) -> f64 { value.to_radians() }
#[inline]
pub fn to_degrees(value: f64) -> f64 { value.to_degrees() }
