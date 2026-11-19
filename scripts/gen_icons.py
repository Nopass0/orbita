"""Генератор векторных иконок Orbita UI.

Что делает
----------
Читает `assets/ui-symbols.svg` (символы, извлечённые из дизайн-системы
Orbita OS: интерфейсные иконки `i-*`, файловые `fi-*`, курсоры `c-*` и
логотип `logo`), раскладывает каждый `<symbol>` на слои (заливка/обводка),
аппроксимирует кривые и дуги ломаными и пишет Rust-модуль
`crates/orbita-ui/src/icons_gen.rs` со статическими массивами точек.

Зачем
-----
Шейдер (GPU) и программный растеризатор (ядро) рисуют иконку прямо из
отрезков: расстояние до ломаной -> обводка любой толщины, чётность
пересечений -> заливка. Никаких растровых атласов, иконка остаётся
чёткой на любом масштабе и при анимациях.

Запуск
------
    python -I scripts/gen_icons.py

Результат детерминирован, сгенерированный файл коммитится в репозиторий.
"""

import math
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "assets", "ui-symbols.svg")
OUT = os.path.join(ROOT, "crates", "orbita-ui", "src", "icons_gen.rs")

# Шаг аппроксимации кривых в единицах viewBox (24 или 32/48).
FLATTEN_TOL = 0.12

# Переменные CSS -> токен дизайн-системы (enum IconColor в Rust).
VAR_TOKENS = {
    "--cursor-fill": "CursorFill",
    "--cursor-outline": "CursorOutline",
    "--cursor-accent-fill": "CursorAccent",
    "--amb-cyan": "AmbCyan",
    "--amb-blue": "AmbBlue",
    "--amb-teal": "AmbTeal",
    "--amb-indigo": "AmbIndigo",
    "--accent-deep": "AccentDeep",
    "--accent": "Accent",
}

# Классы курсоров из bundle.css (fill, stroke, stroke-width).
CLASSES = {
    "cur-f": {"fill": "var(--cursor-fill)", "stroke": "none"},
    "cur-o": {"fill": "none", "stroke": "var(--cursor-outline)", "stroke-width": "1.5"},
    "cur-fo": {"fill": "var(--cursor-fill)", "stroke": "var(--cursor-outline)", "stroke-width": "1.5"},
    "cur-a": {"fill": "var(--cursor-accent-fill)", "stroke": "#fff", "stroke-width": "1.5"},
    "cur-al": {"fill": "none", "stroke": "var(--cursor-accent-fill)", "stroke-width": "2.5"},
    "cur-aw": {"fill": "none", "stroke": "#fff", "stroke-width": "5"},
}

# Градиенты из превью дизайн-системы (url(#id)).
GRADIENTS = {
    "g-fb": ("Linear", ["AccentDeep", "AmbBlue"]),
    "g-ff": ("Linear", ["AmbCyan", "AmbBlue"]),
    "g-im": ("Linear", ["AmbTeal", "AmbBlue"]),
    "g-zip": ("Linear", ["AmbIndigo", "AccentDeep"]),
    "g-lg1": ("Linear", ["AmbCyan", "AmbBlue"]),
    "g-lg2": ("Radial", ["#ffffff", "AmbCyan", "Accent"]),
}


def parse_attrs(text):
    return dict(re.findall(r'([a-zA-Z\-:]+)="([^"]*)"', text))


def style_map(attrs):
    out = {}
    for cls in attrs.get("class", "").split():
        out.update(CLASSES.get(cls, {}))
    for key in ("fill", "stroke", "stroke-width", "opacity", "fill-opacity", "stroke-opacity"):
        if key in attrs:
            out[key] = attrs[key]
    for part in attrs.get("style", "").split(";"):
        if ":" in part:
            k, v = part.split(":", 1)
            out[k.strip()] = v.strip()
    return out


def color_ref(value):
    """CSS-цвет -> выражение Rust `IconColor`."""
    value = value.strip()
    if value in ("currentColor", "inherit", ""):
        return "IconColor::Current"
    m = re.match(r"var\((--[a-z\-]+)\)", value)
    if m:
        return "IconColor::Token(Tok::%s)" % VAR_TOKENS[m.group(1)]
    if value in VAR_TOKENS.values():
        return "IconColor::Token(Tok::%s)" % value
    if value.startswith("#"):
        h = value[1:]
        if len(h) == 3:
            h = "".join(c * 2 for c in h)
        if len(h) == 6:
            h += "ff"
        return "IconColor::Rgba(0x%s)" % h.lower()
    if value == "white":
        return "IconColor::Rgba(0xffffffff)"
    raise ValueError("unknown color " + value)


def paint_ref(value):
    m = re.match(r"url\(#([a-z0-9\-]+)\)", value.strip())
    if m:
        kind, stops = GRADIENTS[m.group(1)]
        refs = [color_ref(s) for s in stops]
        if kind == "Linear":
            return "IconPaint::Linear(%s, %s)" % (refs[0], refs[1])
        return "IconPaint::Radial(%s, %s, %s)" % (refs[0], refs[1], refs[2])
    return "IconPaint::Solid(%s)" % color_ref(value)


# ----------------------------------------------------------------- геометрия

def tokenize_path(d):
    return re.findall(r"[MmLlHhVvCcSsQqTtAaZz]|-?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?", d)


def arc_points(x1, y1, rx, ry, phi_deg, fa, fs, x2, y2):
    """Дуга SVG (endpoint-параметризация) -> список точек (без начальной)."""
    if rx == 0 or ry == 0:
        return [(x2, y2)]
    phi = math.radians(phi_deg)
    cp, sp = math.cos(phi), math.sin(phi)
    dx, dy = (x1 - x2) / 2.0, (y1 - y2) / 2.0
    x1p = cp * dx + sp * dy
    y1p = -sp * dx + cp * dy
    rx, ry = abs(rx), abs(ry)
    lam = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry)
    if lam > 1:
        s = math.sqrt(lam)
        rx *= s
        ry *= s
    num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p
    den = rx * rx * y1p * y1p + ry * ry * x1p * x1p
    coef = math.sqrt(max(0.0, num / den)) if den else 0.0
    if fa == fs:
        coef = -coef
    cxp = coef * rx * y1p / ry
    cyp = -coef * ry * x1p / rx
    cx = cp * cxp - sp * cyp + (x1 + x2) / 2.0
    cy = sp * cxp + cp * cyp + (y1 + y2) / 2.0

    def ang(ux, uy, vx, vy):
        a = math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        return a

    t1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
    dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
    if not fs and dt > 0:
        dt -= 2 * math.pi
    elif fs and dt < 0:
        dt += 2 * math.pi
    steps = max(4, int(abs(dt) * max(rx, ry) / (FLATTEN_TOL * 6)) + 1)
    steps = min(steps, 48)
    pts = []
    for i in range(1, steps + 1):
        t = t1 + dt * i / steps
        x = cx + rx * math.cos(t) * cp - ry * math.sin(t) * sp
        y = cy + rx * math.cos(t) * sp + ry * math.sin(t) * cp
        pts.append((x, y))
    return pts


def cubic_points(p0, p1, p2, p3):
    length = math.dist(p0, p1) + math.dist(p1, p2) + math.dist(p2, p3)
    steps = max(3, min(32, int(length / (FLATTEN_TOL * 5)) + 1))
    pts = []
    for i in range(1, steps + 1):
        t = i / steps
        mt = 1 - t
        x = mt**3 * p0[0] + 3 * mt * mt * t * p1[0] + 3 * mt * t * t * p2[0] + t**3 * p3[0]
        y = mt**3 * p0[1] + 3 * mt * mt * t * p1[1] + 3 * mt * t * t * p2[1] + t**3 * p3[1]
        pts.append((x, y))
    return pts


def path_contours(d):
    """Строка `d` -> список (closed, [точки])."""
    toks = tokenize_path(d)
    i = 0
    contours = []
    cur = []
    x = y = 0.0
    sx = sy = 0.0
    last_ctrl = None
    cmd = None

    def num():
        nonlocal i
        v = float(toks[i])
        i += 1
        return v

    def flush(closed):
        nonlocal cur
        if len(cur) > 1:
            contours.append((closed, cur))
        cur = []

    while i < len(toks):
        t = toks[i]
        if re.match(r"[A-Za-z]", t):
            cmd = t
            i += 1
            if cmd in "Zz":
                if cur:
                    cur.append((sx, sy))
                flush(True)
                x, y = sx, sy
                cur = []
                continue
        rel = cmd.islower()
        c = cmd.upper()
        if c == "M":
            nx, ny = num(), num()
            if rel:
                nx += x
                ny += y
            flush(False)
            x, y = nx, ny
            sx, sy = x, y
            cur = [(x, y)]
            cmd = "l" if rel else "L"
            last_ctrl = None
        elif c == "L":
            nx, ny = num(), num()
            if rel:
                nx += x
                ny += y
            x, y = nx, ny
            cur.append((x, y))
            last_ctrl = None
        elif c == "H":
            nx = num()
            x = nx + x if rel else nx
            cur.append((x, y))
            last_ctrl = None
        elif c == "V":
            ny = num()
            y = ny + y if rel else ny
            cur.append((x, y))
            last_ctrl = None
        elif c in "CS":
            if c == "C":
                c1 = (num(), num())
                if rel:
                    c1 = (c1[0] + x, c1[1] + y)
            else:
                c1 = (2 * x - last_ctrl[0], 2 * y - last_ctrl[1]) if last_ctrl else (x, y)
            c2 = (num(), num())
            p = (num(), num())
            if rel:
                c2 = (c2[0] + x, c2[1] + y)
                p = (p[0] + x, p[1] + y)
            if not cur:
                cur = [(x, y)]
            cur.extend(cubic_points((x, y), c1, c2, p))
            last_ctrl = c2
            x, y = p
        elif c == "Q":
            q1 = (num(), num())
            p = (num(), num())
            if rel:
                q1 = (q1[0] + x, q1[1] + y)
                p = (p[0] + x, p[1] + y)
            c1 = (x + 2 / 3 * (q1[0] - x), y + 2 / 3 * (q1[1] - y))
            c2 = (p[0] + 2 / 3 * (q1[0] - p[0]), p[1] + 2 / 3 * (q1[1] - p[1]))
            cur.extend(cubic_points((x, y), c1, c2, p))
            last_ctrl = None
            x, y = p
        elif c == "A":
            rx, ry, rot = num(), num(), num()
            fa, fs = int(num()), int(num())
            nx, ny = num(), num()
            if rel:
                nx += x
                ny += y
            if not cur:
                cur = [(x, y)]
            cur.extend(arc_points(x, y, rx, ry, rot, fa, fs, nx, ny))
            x, y = nx, ny
            last_ctrl = None
        else:
            raise ValueError("unsupported command " + cmd)
    flush(False)
    return contours


def circle_contour(cx, cy, r):
    steps = max(16, min(64, int(2 * math.pi * r / (FLATTEN_TOL * 4))))
    pts = [(cx + r * math.cos(2 * math.pi * i / steps), cy + r * math.sin(2 * math.pi * i / steps)) for i in range(steps + 1)]
    return [(True, pts)]


def rect_contour(x, y, w, h, rx):
    if rx <= 0:
        return [(True, [(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)])]
    d = "M%f %fh%fa%f %f 0 0 1 %f %fv%fa%f %f 0 0 1 %f %fh%fa%f %f 0 0 1 %f %fv%fa%f %f 0 0 1 %f %fz" % (
        x + rx, y, w - 2 * rx, rx, rx, rx, rx, h - 2 * rx, rx, rx, -rx, rx, -(w - 2 * rx), rx, rx, -rx, -rx,
        -(h - 2 * rx), rx, rx, rx, -rx)
    return path_contours(d)


def apply_transform(contours, transform):
    if not transform:
        return contours
    tx = ty = 0.0
    rot = 0.0
    m = re.search(r"translate\(([-\d.]+)[ ,]+([-\d.]+)\)", transform)
    if m:
        tx, ty = float(m.group(1)), float(m.group(2))
    m = re.search(r"rotate\(([-\d.]+)\)", transform)
    if m:
        rot = math.radians(float(m.group(1)))
    c, s = math.cos(rot), math.sin(rot)
    out = []
    for closed, pts in contours:
        out.append((closed, [(tx + c * px - s * py, ty + s * px + c * py) for px, py in pts]))
    return out


# --------------------------------------------------------------- генерация

def parse_symbols():
    text = open(SRC, encoding="utf8").read()
    symbols = []
    for m in re.finditer(r'<symbol id="([^"]+)" viewBox="0 0 (\d+) (\d+)">(.*?)</symbol>', text, re.S):
        sid, vb, body = m.group(1), float(m.group(2)), m.group(4)
        symbols.append((sid, vb, body))
    return symbols


def element_layers(sid, body):
    """Разбирает тело символа на слои (mode, paint, opacity, width, contours)."""
    layers = []
    transform = None
    g = re.search(r'<g transform="([^"]+)">', body)
    if g:
        transform = g.group(1)
    for em in re.finditer(r"<(path|circle|rect)\b([^>]*?)/?>", body):
        tag, attrs = em.group(1), parse_attrs(em.group(2))
        st = style_map(attrs)
        if tag == "path":
            contours = path_contours(attrs["d"])
        elif tag == "circle":
            contours = circle_contour(float(attrs.get("cx", 0)), float(attrs.get("cy", 0)), float(attrs["r"]))
        else:
            contours = rect_contour(float(attrs.get("x", 0)), float(attrs.get("y", 0)), float(attrs["width"]),
                                    float(attrs["height"]), float(attrs.get("rx", 0)))
        contours = apply_transform(contours, transform)
        opacity = float(st.get("opacity", "1"))
        is_ui_icon = sid.startswith("i-")
        fill = st.get("fill", "none" if is_ui_icon else "#000")
        stroke = st.get("stroke", "currentColor" if is_ui_icon else "none")
        width = float(st.get("stroke-width", "1.75" if is_ui_icon else "1"))
        if fill != "none":
            op = opacity * float(st.get("fill-opacity", "1"))
            layers.append(("Fill", paint_ref(fill), op, 0.0, contours))
        if stroke != "none":
            op = opacity * float(st.get("stroke-opacity", "1"))
            layers.append(("Stroke", paint_ref(stroke), op, width, contours))
    return layers


def rust_ident(sid):
    parts = re.split(r"[-]", sid)
    return "".join(p.capitalize() for p in parts)


def main():
    symbols = parse_symbols()
    points = []
    out = []
    out.append("// @generated by scripts/gen_icons.py — не редактировать вручную.")
    out.append("//! Векторная геометрия иконок дизайн-системы Orbita OS (сгенерировано).")
    out.append("//!")
    out.append("//! Каждая иконка — набор слоёв [`IconLayer`]: заливка или обводка ломаных")
    out.append("//! в координатах viewBox. Точки всех иконок лежат в общем массиве")
    out.append("//! [`ICON_POINTS`], контур кодируется диапазоном индексов.")
    out.append("")
    out.append("use crate::icons::{IconColor, IconDef, IconLayer, IconPaint, LayerMode, Tok};")
    out.append("")
    defs = []
    for sid, vb, body in symbols:
        layers_rs = []
        for mode, paint, op, width, contours in element_layers(sid, body):
            cont_rs = []
            for closed, pts in contours:
                start = len(points)
                # Удаляем подряд идущие дубликаты.
                clean = []
                for p in pts:
                    if not clean or math.dist(clean[-1], p) > 1e-4:
                        clean.append(p)
                if len(clean) < 2:
                    continue
                points.extend(clean)
                cont_rs.append("(%d, %d)" % (start, len(points)))
            if not cont_rs:
                continue
            layers_rs.append(
                "IconLayer { mode: LayerMode::%s, paint: %s, opacity: %.3f, width: %.3f, contours: &[%s] }"
                % (mode, paint, op, width, ", ".join(cont_rs)))
        defs.append((sid, vb, layers_rs))

    out.append("/// Все точки всех иконок (x, y в единицах viewBox соответствующей иконки).")
    out.append("pub static ICON_POINTS: [[f32; 2]; %d] = [" % len(points))
    for i in range(0, len(points), 6):
        chunk = points[i:i + 6]
        out.append("    " + " ".join("[%.3f, %.3f]," % (px, py) for px, py in chunk))
    out.append("];")
    out.append("")
    out.append("/// Идентификаторы иконок (имена совпадают с символами дизайн-системы).")
    out.append("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]")
    out.append("pub enum Icon {")
    for sid, _, _ in defs:
        out.append("    /// Символ `%s` дизайн-системы." % sid)
        out.append("    %s," % rust_ident(sid))
    out.append("}")
    out.append("")
    out.append("impl Icon {")
    out.append("    /// Все иконки в порядке объявления.")
    out.append("    pub const ALL: [Icon; %d] = [%s];" % (len(defs), ", ".join("Icon::" + rust_ident(s) for s, _, _ in defs)))
    out.append("")
    out.append("    /// Описание геометрии иконки.")
    out.append("    pub fn def(self) -> &'static IconDef {")
    out.append("        &ICON_DEFS[self as usize]")
    out.append("    }")
    out.append("}")
    out.append("")
    out.append("/// Таблица геометрии, индекс = `Icon as usize`.")
    out.append("pub static ICON_DEFS: [IconDef; %d] = [" % len(defs))
    for sid, vb, layers_rs in defs:
        out.append("    IconDef { name: \"%s\", view: %.1f, layers: &[" % (sid, vb))
        for l in layers_rs:
            out.append("        " + l + ",")
        out.append("    ] },")
    out.append("];")
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf8", newline="\n") as f:
        f.write("\n".join(out) + "\n")
    print("icons: %d, points: %d -> %s" % (len(defs), len(points), OUT))


if __name__ == "__main__":
    sys.exit(main())
