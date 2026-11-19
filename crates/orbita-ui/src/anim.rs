//! Движок анимаций: кубические кривые Безье (как CSS `cubic-bezier`),
//! твины с целевым значением и пружины.
//!
//! Все анимации считаются от абсолютного времени кадра (секунды), поэтому
//! плавность не зависит от частоты кадров: при 60, 160 или 300 FPS окно
//! открывается одинаковые 420 мс, просто с большим числом промежуточных
//! состояний.

use hashbrown::HashMap;

use crate::math;

/// Кубическая кривая Безье `cubic-bezier(x1, y1, x2, y2)` с концами (0,0)–(1,1).
///
/// Решение `x(t) = progress` — методом Ньютона с откатом на бисекцию
/// (тот же алгоритм, что в браузерных движках).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier {
    cx: f32,
    bx: f32,
    ax: f32,
    cy: f32,
    by: f32,
    ay: f32,
}

impl CubicBezier {
    /// Создаёт кривую по двум контрольным точкам.
    ///
    /// ```
    /// use orbita_ui::anim::CubicBezier;
    /// let ease = CubicBezier::new(0.22, 0.8, 0.24, 1.0);
    /// assert_eq!(ease.eval(0.0), 0.0);
    /// assert!((ease.eval(1.0) - 1.0).abs() < 1e-4);
    /// assert!(ease.eval(0.5) > 0.5); // ease-out: к середине уже больше половины пути
    /// ```
    pub fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> CubicBezier {
        let cx = 3.0 * x1;
        let bx = 3.0 * (x2 - x1) - cx;
        let ax = 1.0 - cx - bx;
        let cy = 3.0 * y1;
        let by = 3.0 * (y2 - y1) - cy;
        let ay = 1.0 - cy - by;
        CubicBezier { cx, bx, ax, cy, by, ay }
    }

    /// Кривая из массива токена (`motion::EASE`).
    pub fn from_array(v: [f32; 4]) -> CubicBezier {
        CubicBezier::new(v[0], v[1], v[2], v[3])
    }

    fn sample_x(&self, t: f32) -> f32 {
        ((self.ax * t + self.bx) * t + self.cx) * t
    }

    fn sample_y(&self, t: f32) -> f32 {
        ((self.ay * t + self.by) * t + self.cy) * t
    }

    fn sample_dx(&self, t: f32) -> f32 {
        (3.0 * self.ax * t + 2.0 * self.bx) * t + self.cx
    }

    fn solve_x(&self, x: f32) -> f32 {
        // Ньютон: быстро сходится на гладких участках.
        let mut t = x;
        for _ in 0..8 {
            let err = self.sample_x(t) - x;
            if math::abs(err) < 1e-5 {
                return t;
            }
            let d = self.sample_dx(t);
            if math::abs(d) < 1e-6 {
                break;
            }
            t -= err / d;
        }
        // Бисекция: гарантированный результат.
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        t = x;
        for _ in 0..24 {
            let v = self.sample_x(t);
            if math::abs(v - x) < 1e-5 {
                break;
            }
            if x > v {
                lo = t;
            } else {
                hi = t;
            }
            t = (lo + hi) * 0.5;
        }
        t
    }

    /// Значение кривой для прогресса `x ∈ [0, 1]`.
    ///
    /// Принимает: доля прошедшего времени.
    /// Возвращает: доля пройденного пути (может слегка выходить за 0..1
    /// у «пружинящих» кривых).
    pub fn eval(&self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        self.sample_y(self.solve_x(x))
    }
}

/// Кривые дизайн-системы.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ease {
    /// Линейно.
    Linear,
    /// `--ob-ease` — основное выразительное замедление.
    Out,
    /// `--ob-ease-soft` — мягкий ease-in-out для цвета.
    Soft,
}

impl Ease {
    /// Применяет кривую к прогрессу `t ∈ [0, 1]`.
    ///
    /// ```
    /// use orbita_ui::anim::Ease;
    /// assert_eq!(Ease::Linear.apply(0.25), 0.25);
    /// assert!(Ease::Out.apply(0.25) > 0.25);
    /// ```
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Ease::Linear => math::clamp(t, 0.0, 1.0),
            Ease::Out => out_curve().eval(t),
            Ease::Soft => soft_curve().eval(t),
        }
    }
}

fn out_curve() -> CubicBezier {
    CubicBezier::new(0.22, 0.8, 0.24, 1.0)
}

fn soft_curve() -> CubicBezier {
    CubicBezier::new(0.4, 0.0, 0.2, 1.0)
}

/// Твин: значение, плавно идущее от `from` к `to` за `duration` секунд.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween {
    /// Начальное значение текущего перехода.
    pub from: f32,
    /// Целевое значение.
    pub to: f32,
    /// Время старта перехода (сек).
    pub start: f64,
    /// Длительность (сек).
    pub duration: f32,
    /// Кривая.
    pub ease: Ease,
}

impl Tween {
    /// Неподвижное значение (без перехода).
    ///
    /// ```
    /// use orbita_ui::anim::Tween;
    /// let t = Tween::fixed(1.0);
    /// assert_eq!(t.value(100.0), 1.0);
    /// ```
    pub fn fixed(v: f32) -> Tween {
        Tween { from: v, to: v, start: 0.0, duration: 0.0, ease: Ease::Linear }
    }

    /// Значение в момент времени `now`.
    pub fn value(&self, now: f64) -> f32 {
        if self.duration <= 0.0 {
            return self.to;
        }
        let p = ((now - self.start) as f32) / self.duration;
        if p >= 1.0 {
            return self.to;
        }
        let k = self.ease.apply(math::max(p, 0.0));
        math::lerp(self.from, self.to, k)
    }

    /// Завершён ли переход к моменту `now`.
    pub fn done(&self, now: f64) -> bool {
        self.duration <= 0.0 || (now - self.start) as f32 >= self.duration
    }

    /// Запускает переход к новой цели из текущего значения (без скачков).
    ///
    /// ```
    /// use orbita_ui::anim::{Ease, Tween};
    /// let mut t = Tween::fixed(0.0);
    /// t.retarget(1.0, 0.0, 0.5, Ease::Linear);
    /// assert!((t.value(0.25) - 0.5).abs() < 1e-5);
    /// ```
    pub fn retarget(&mut self, to: f32, now: f64, duration: f32, ease: Ease) {
        if math::abs(self.to - to) < 1e-6 {
            return;
        }
        let current = self.value(now);
        *self = Tween { from: current, to, start: now, duration, ease };
    }
}

/// Критически демпфированная пружина (плавное «догоняние» цели без перелёта).
///
/// Используется для перетаскивания (окна, ползунки) и прокрутки: значение
/// следует за курсором с естественной инерцией, не зависящей от FPS.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Spring {
    /// Текущее значение.
    pub value: f32,
    /// Текущая скорость.
    pub velocity: f32,
}

impl Spring {
    /// Создаёт пружину в состоянии покоя.
    pub const fn new(value: f32) -> Spring {
        Spring { value, velocity: 0.0 }
    }

    /// Шаг симуляции.
    ///
    /// Принимает: `target` — цель, `dt` — шаг времени (сек), `omega` —
    /// жёсткость (рад/с, 20–40 — отзывчиво).
    /// Возвращает: новое значение.
    ///
    /// ```
    /// use orbita_ui::anim::Spring;
    /// let mut s = Spring::new(0.0);
    /// for _ in 0..200 { s.step(100.0, 1.0 / 160.0, 30.0); }
    /// assert!((s.value - 100.0).abs() < 0.5);
    /// ```
    pub fn step(&mut self, target: f32, dt: f32, omega: f32) -> f32 {
        // Аналитическое решение критически демпфированного осциллятора —
        // устойчиво при любом dt (в отличие от явного Эйлера).
        let dt = math::clamp(dt, 0.0, 0.1);
        let x = self.value - target;
        let e = math::exp(-omega * dt);
        let new_x = (x + (self.velocity + omega * x) * dt) * e;
        self.velocity = (self.velocity - omega * (self.velocity + omega * x) * dt) * e;
        self.value = target + new_x;
        if math::abs(new_x) < 0.01 && math::abs(self.velocity) < 0.05 {
            self.value = target;
            self.velocity = 0.0;
        }
        self.value
    }

    /// Покоится ли пружина в цели.
    pub fn settled(&self, target: f32) -> bool {
        self.value == target && self.velocity == 0.0
    }
}

/// Хранилище анимаций по ключу: каждый виджет просит «значение, идущее к
/// цели», а хранилище само помнит, откуда и когда начался переход.
#[derive(Default)]
pub struct Anims {
    tracks: HashMap<u64, Tween>,
    /// Множитель длительностей: 1 — как в дизайне, 0 — анимации выключены.
    pub speed: f32,
    active: bool,
}

impl Anims {
    /// Пустое хранилище (анимации включены).
    pub fn new() -> Anims {
        Anims { tracks: HashMap::new(), speed: 1.0, active: false }
    }

    /// Значение анимации `key`, плавно идущей к `target`.
    ///
    /// Принимает: `key` — стабильный идентификатор, `target` — цель,
    /// `duration` — длительность перехода, `ease` — кривая, `now` — время кадра.
    /// Возвращает: текущее значение. При первом обращении — сразу `target`.
    ///
    /// ```
    /// use orbita_ui::anim::{Anims, Ease};
    /// let mut a = Anims::new();
    /// assert_eq!(a.to(1, 0.0, 0.2, Ease::Linear, 0.0), 0.0);
    /// a.to(1, 1.0, 0.2, Ease::Linear, 1.0); // старт перехода
    /// let mid = a.to(1, 1.0, 0.2, Ease::Linear, 1.1);
    /// assert!((mid - 0.5).abs() < 1e-4);
    /// ```
    pub fn to(&mut self, key: u64, target: f32, duration: f32, ease: Ease, now: f64) -> f32 {
        let dur = duration * self.speed;
        let tr = self.tracks.entry(key).or_insert_with(|| Tween::fixed(target));
        tr.retarget(target, now, dur, ease);
        if !tr.done(now) {
            self.active = true;
        }
        tr.value(now)
    }

    /// Начать анимацию «с нуля»: значение мгновенно ставится в `from`
    /// и идёт к `to` (для появлений: окно, меню, тост).
    pub fn start(&mut self, key: u64, from: f32, to: f32, duration: f32, ease: Ease, now: f64) {
        self.tracks.insert(
            key,
            Tween { from, to, start: now, duration: duration * self.speed, ease },
        );
        self.active = true;
    }

    /// Текущее значение без изменения цели (`None`, если ключа нет).
    pub fn get(&self, key: u64, now: f64) -> Option<f32> {
        self.tracks.get(&key).map(|t| t.value(now))
    }

    /// Завершён ли переход ключа (отсутствующий считается завершённым).
    pub fn finished(&self, key: u64, now: f64) -> bool {
        self.tracks.get(&key).map(|t| t.done(now)).unwrap_or(true)
    }

    /// Удаляет анимацию.
    pub fn remove(&mut self, key: u64) {
        self.tracks.remove(&key);
    }

    /// Были ли незавершённые переходы в этом кадре (нужен ли следующий кадр).
    ///
    /// Сбрасывает флаг — вызывается один раз в конце кадра.
    pub fn take_active(&mut self) -> bool {
        core::mem::replace(&mut self.active, false)
    }

    /// Отмечает, что в кадре идёт анимация, которой нет в хранилище
    /// (пружины, мигание курсора).
    pub fn mark_active(&mut self) {
        self.active = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_is_monotonic_for_design_curves() {
        for curve in [out_curve(), soft_curve()] {
            let mut prev = 0.0;
            for i in 0..=100 {
                let v = curve.eval(i as f32 / 100.0);
                assert!(v + 1e-4 >= prev, "curve must not go backwards");
                prev = v;
            }
            assert!((prev - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn tween_retarget_keeps_continuity() {
        let mut t = Tween::fixed(0.0);
        t.retarget(10.0, 0.0, 1.0, Ease::Linear);
        let at = t.value(0.5);
        t.retarget(0.0, 0.5, 1.0, Ease::Linear);
        assert!((t.value(0.5) - at).abs() < 1e-4);
    }

    #[test]
    fn anims_speed_zero_is_instant() {
        let mut a = Anims::new();
        a.speed = 0.0;
        a.to(7, 0.0, 1.0, Ease::Out, 0.0);
        assert_eq!(a.to(7, 1.0, 1.0, Ease::Out, 0.01), 1.0);
    }
}
