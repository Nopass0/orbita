//! Математика для `no_std`: тонкие обёртки над `libm`.
//!
//! В `core` у `f32` нет `sqrt`, `sin`, `exp` и т.п. (они живут в `std`),
//! поэтому весь крейт пользуется этими функциями. На хосте и в ядре они
//! дают одинаковый результат — анимации и раскладка детерминированы.

/// Квадратный корень.
///
/// Принимает: `x` — неотрицательное число.
/// Возвращает: `√x` (для отрицательных — `NaN`).
///
/// ```
/// assert_eq!(orbita_ui::math::sqrt(9.0), 3.0);
/// ```
#[inline]
pub fn sqrt(x: f32) -> f32 {
    libm::sqrtf(x)
}

/// Экспонента `e^x`.
///
/// ```
/// assert!((orbita_ui::math::exp(0.0) - 1.0).abs() < 1e-6);
/// ```
#[inline]
pub fn exp(x: f32) -> f32 {
    libm::expf(x)
}

/// Степень `x^y`.
///
/// ```
/// assert_eq!(orbita_ui::math::pow(2.0, 3.0), 8.0);
/// ```
#[inline]
pub fn pow(x: f32, y: f32) -> f32 {
    libm::powf(x, y)
}

/// Синус (радианы).
#[inline]
pub fn sin(x: f32) -> f32 {
    libm::sinf(x)
}

/// Косинус (радианы).
#[inline]
pub fn cos(x: f32) -> f32 {
    libm::cosf(x)
}

/// Арктангенс `atan2(y, x)` в радианах, диапазон `(-π, π]`.
#[inline]
pub fn atan2(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

/// Округление вниз.
///
/// ```
/// assert_eq!(orbita_ui::math::floor(1.7), 1.0);
/// assert_eq!(orbita_ui::math::floor(-1.2), -2.0);
/// ```
#[inline]
pub fn floor(x: f32) -> f32 {
    libm::floorf(x)
}

/// Округление вверх.
#[inline]
pub fn ceil(x: f32) -> f32 {
    libm::ceilf(x)
}

/// Округление к ближайшему целому (половины — от нуля).
///
/// ```
/// assert_eq!(orbita_ui::math::round(2.5), 3.0);
/// ```
#[inline]
pub fn round(x: f32) -> f32 {
    libm::roundf(x)
}

/// Модуль числа.
#[inline]
pub fn abs(x: f32) -> f32 {
    libm::fabsf(x)
}

/// Двоичный логарифм.
#[inline]
pub fn log2(x: f32) -> f32 {
    libm::log2f(x)
}

/// Ограничивает `x` отрезком `[lo, hi]`.
///
/// ```
/// assert_eq!(orbita_ui::math::clamp(5.0, 0.0, 1.0), 1.0);
/// ```
#[inline]
pub fn clamp(x: f32, lo: f32, hi: f32) -> f32 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

/// Линейная интерполяция `a + (b - a) * t`.
///
/// ```
/// assert_eq!(orbita_ui::math::lerp(10.0, 20.0, 0.5), 15.0);
/// ```
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Гладкая ступень Эрмита: 0 при `x <= e0`, 1 при `x >= e1`.
///
/// Используется для сглаживания краёв (антиалиасинг SDF).
///
/// ```
/// assert_eq!(orbita_ui::math::smoothstep(0.0, 1.0, 0.5), 0.5);
/// ```
#[inline]
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Максимум из двух чисел (без `NaN`-магии `f32::max`).
#[inline]
pub fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

/// Минимум из двух чисел.
#[inline]
pub fn min(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}
