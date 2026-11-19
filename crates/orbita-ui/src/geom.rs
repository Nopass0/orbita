//! Геометрия интерфейса: точки, прямоугольники, радиусы скругления.
//!
//! Все координаты — логические пиксели (CSS px). Перевод в физические
//! пиксели экрана делает [`crate::gfx::Painter`] через коэффициент `scale`.

use crate::math;

/// Двумерный вектор / точка.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec2 {
    /// Координата X (вправо).
    pub x: f32,
    /// Координата Y (вниз).
    pub y: f32,
}

impl Vec2 {
    /// Нулевой вектор.
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    /// Создаёт вектор.
    ///
    /// ```
    /// let p = orbita_ui::Vec2::new(3.0, 4.0);
    /// assert_eq!(p.length(), 5.0);
    /// ```
    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

    /// Длина вектора.
    pub fn length(self) -> f32 {
        math::sqrt(self.x * self.x + self.y * self.y)
    }

    /// Сумма векторов.
    pub fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }

    /// Разность векторов.
    pub fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }

    /// Умножение на число.
    pub fn scale(self, k: f32) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }

    /// Линейная интерполяция между двумя точками.
    pub fn lerp(self, o: Vec2, t: f32) -> Vec2 {
        Vec2::new(math::lerp(self.x, o.x, t), math::lerp(self.y, o.y, t))
    }
}

/// Прямоугольник: левый верхний угол и размер.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    /// Левый край.
    pub x: f32,
    /// Верхний край.
    pub y: f32,
    /// Ширина (≥ 0).
    pub w: f32,
    /// Высота (≥ 0).
    pub h: f32,
}

impl Rect {
    /// Пустой прямоугольник в начале координат.
    pub const ZERO: Rect = Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };

    /// Создаёт прямоугольник.
    ///
    /// ```
    /// let r = orbita_ui::Rect::new(10.0, 20.0, 100.0, 50.0);
    /// assert_eq!(r.right(), 110.0);
    /// ```
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    /// Создаёт прямоугольник по двум углам (min, max).
    pub fn from_min_max(min: Vec2, max: Vec2) -> Rect {
        Rect::new(
            math::min(min.x, max.x),
            math::min(min.y, max.y),
            math::abs(max.x - min.x),
            math::abs(max.y - min.y),
        )
    }

    /// Правый край.
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// Нижний край.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Центр прямоугольника.
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    /// Содержит ли прямоугольник точку (правый/нижний край не включается).
    ///
    /// ```
    /// use orbita_ui::{Rect, Vec2};
    /// let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    /// assert!(r.contains(Vec2::new(5.0, 5.0)));
    /// assert!(!r.contains(Vec2::new(10.0, 5.0)));
    /// ```
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    /// Уменьшает прямоугольник на `d` со всех сторон (отрицательное — увеличивает).
    ///
    /// ```
    /// let r = orbita_ui::Rect::new(0.0, 0.0, 10.0, 10.0).inset(2.0);
    /// assert_eq!(r, orbita_ui::Rect::new(2.0, 2.0, 6.0, 6.0));
    /// ```
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(
            self.x + d,
            self.y + d,
            math::max(0.0, self.w - 2.0 * d),
            math::max(0.0, self.h - 2.0 * d),
        )
    }

    /// Уменьшает прямоугольник отдельно по горизонтали (`dx`) и вертикали (`dy`).
    pub fn inset_xy(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(
            self.x + dx,
            self.y + dy,
            math::max(0.0, self.w - 2.0 * dx),
            math::max(0.0, self.h - 2.0 * dy),
        )
    }

    /// Пересечение двух прямоугольников (пустое — нулевого размера).
    ///
    /// ```
    /// use orbita_ui::Rect;
    /// let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    /// let b = Rect::new(5.0, 5.0, 10.0, 10.0);
    /// assert_eq!(a.intersect(&b), Rect::new(5.0, 5.0, 5.0, 5.0));
    /// ```
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = math::max(self.x, o.x);
        let y0 = math::max(self.y, o.y);
        let x1 = math::min(self.right(), o.right());
        let y1 = math::min(self.bottom(), o.bottom());
        Rect::new(x0, y0, math::max(0.0, x1 - x0), math::max(0.0, y1 - y0))
    }

    /// Наименьший прямоугольник, содержащий оба.
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x0 = math::min(self.x, o.x);
        let y0 = math::min(self.y, o.y);
        let x1 = math::max(self.right(), o.right());
        let y1 = math::max(self.bottom(), o.bottom());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Пустой ли прямоугольник (нулевая площадь).
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// Сдвигает прямоугольник на вектор.
    pub fn translate(&self, d: Vec2) -> Rect {
        Rect::new(self.x + d.x, self.y + d.y, self.w, self.h)
    }

    /// Масштабирует прямоугольник относительно точки `origin`.
    ///
    /// ```
    /// use orbita_ui::{Rect, Vec2};
    /// let r = Rect::new(0.0, 0.0, 10.0, 10.0).scale_about(Vec2::new(5.0, 5.0), 2.0);
    /// assert_eq!(r, Rect::new(-5.0, -5.0, 20.0, 20.0));
    /// ```
    pub fn scale_about(&self, origin: Vec2, k: f32) -> Rect {
        Rect::new(
            origin.x + (self.x - origin.x) * k,
            origin.y + (self.y - origin.y) * k,
            self.w * k,
            self.h * k,
        )
    }

    /// Интерполяция прямоугольников (для анимаций разворачивания окна).
    pub fn lerp(&self, o: &Rect, t: f32) -> Rect {
        Rect::new(
            math::lerp(self.x, o.x, t),
            math::lerp(self.y, o.y, t),
            math::lerp(self.w, o.w, t),
            math::lerp(self.h, o.h, t),
        )
    }

    /// Отрезает полосу сверху высотой `h`; возвращает (полоса, остаток).
    ///
    /// ```
    /// let (top, rest) = orbita_ui::Rect::new(0.0, 0.0, 100.0, 100.0).split_top(30.0);
    /// assert_eq!(top.h, 30.0);
    /// assert_eq!(rest.y, 30.0);
    /// ```
    pub fn split_top(&self, h: f32) -> (Rect, Rect) {
        let h = math::min(h, self.h);
        (
            Rect::new(self.x, self.y, self.w, h),
            Rect::new(self.x, self.y + h, self.w, self.h - h),
        )
    }

    /// Отрезает полосу снизу высотой `h`; возвращает (остаток, полоса).
    pub fn split_bottom(&self, h: f32) -> (Rect, Rect) {
        let h = math::min(h, self.h);
        (
            Rect::new(self.x, self.y, self.w, self.h - h),
            Rect::new(self.x, self.bottom() - h, self.w, h),
        )
    }

    /// Отрезает полосу слева шириной `w`; возвращает (полоса, остаток).
    pub fn split_left(&self, w: f32) -> (Rect, Rect) {
        let w = math::min(w, self.w);
        (
            Rect::new(self.x, self.y, w, self.h),
            Rect::new(self.x + w, self.y, self.w - w, self.h),
        )
    }

    /// Отрезает полосу справа шириной `w`; возвращает (остаток, полоса).
    pub fn split_right(&self, w: f32) -> (Rect, Rect) {
        let w = math::min(w, self.w);
        (
            Rect::new(self.x, self.y, self.w - w, self.h),
            Rect::new(self.right() - w, self.y, w, self.h),
        )
    }

    /// Прямоугольник заданного размера, отцентрованный внутри `self`.
    pub fn center_box(&self, w: f32, h: f32) -> Rect {
        Rect::new(self.x + (self.w - w) * 0.5, self.y + (self.h - h) * 0.5, w, h)
    }

    /// Округляет координаты до целых пикселей (чёткие края).
    pub fn snap(&self) -> Rect {
        let x0 = math::round(self.x);
        let y0 = math::round(self.y);
        Rect::new(x0, y0, math::round(self.right()) - x0, math::round(self.bottom()) - y0)
    }
}

/// Радиусы скругления углов: левый-верхний, правый-верхний, правый-нижний, левый-нижний.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Radii {
    /// Левый верхний.
    pub tl: f32,
    /// Правый верхний.
    pub tr: f32,
    /// Правый нижний.
    pub br: f32,
    /// Левый нижний.
    pub bl: f32,
}

impl Radii {
    /// Без скругления.
    pub const ZERO: Radii = Radii { tl: 0.0, tr: 0.0, br: 0.0, bl: 0.0 };

    /// Одинаковый радиус для всех углов.
    ///
    /// ```
    /// let r = orbita_ui::Radii::all(12.0);
    /// assert_eq!(r.br, 12.0);
    /// ```
    pub const fn all(r: f32) -> Radii {
        Radii { tl: r, tr: r, br: r, bl: r }
    }

    /// Скругление только верхних углов (вкладки, заголовки).
    pub const fn top(r: f32) -> Radii {
        Radii { tl: r, tr: r, br: 0.0, bl: 0.0 }
    }

    /// Скругление только нижних углов.
    pub const fn bottom(r: f32) -> Radii {
        Radii { tl: 0.0, tr: 0.0, br: r, bl: r }
    }

    /// Умножает все радиусы на коэффициент.
    pub fn scale(self, k: f32) -> Radii {
        Radii { tl: self.tl * k, tr: self.tr * k, br: self.br * k, bl: self.bl * k }
    }

    /// Ограничивает радиусы половиной меньшей стороны (как браузер для `9999px`).
    pub fn clamp_to(self, w: f32, h: f32) -> Radii {
        let m = math::max(0.0, math::min(w, h) * 0.5);
        Radii {
            tl: math::min(self.tl, m),
            tr: math::min(self.tr, m),
            br: math::min(self.br, m),
            bl: math::min(self.bl, m),
        }
    }

    /// Массив `[tl, tr, br, bl]` для GPU.
    pub fn to_array(self) -> [f32; 4] {
        [self.tl, self.tr, self.br, self.bl]
    }
}

impl From<f32> for Radii {
    fn from(r: f32) -> Radii {
        Radii::all(r)
    }
}
