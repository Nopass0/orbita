//! Шейдеры TGSI для virgl.
//!
//! virgl передаёт шейдеры текстом TGSI (ассемблер Gallium); virglrenderer
//! на хосте переводит их в GLSL и компилирует драйвером реальной видеокарты.
//! Тексты собираются из Rust-функций, чтобы повторяющиеся куски (SDF
//! скруглённого прямоугольника) не копировались вручную.
//!
//! ## UI-шейдер
//!
//! Вершина несёт все 13 полей [`orbita_ui::gfx::Instance`] (плюс позицию
//! пикселя). Фрагментный шейдер без ветвлений считает результат для всех
//! видов примитива и выбирает нужный по `params.x` — та же математика,
//! что в программном растеризаторе `orbita_ui::gfx::soft`:
//!
//! | вид | что считает |
//! |---|---|
//! | 0 Rect | SDF + градиент (линейный/радиальный, 3 точки) + рамка + световая кромка |
//! | 1 Shadow | размытый прямоугольник через erf, маска «не под элементом» |
//! | 2 Glyph | покрытие из атласа R8 × цвет |
//! | 3 Sprite | предумноженный RGBA из атласа спрайтов (иконки, курсоры) |
//! | 4 Backdrop | размытый снимок сцены (1/8) + насыщенность — стекло |
//! | 5 Noise | статичное зерно |
//! | 6 Image | текстура обоев |
//!
//! В конце результат умножается на покрытие прямоугольника отсечения
//! (со скруглением окна).

extern crate alloc;

use alloc::format;
use alloc::string::String;

/// Число vec4-атрибутов вершины UI: позиция + 12 полей инстанса.
pub const UI_ATTRS: u32 = 13;

/// SDF скруглённого прямоугольника.
///
/// Принимает: регистр-приёмник (`TEMP[5].x`), точку, прямоугольник
/// `x,y,w,h`, радиусы `tl,tr,br,bl`. Портит `TEMP[1..4]`.
fn sdf(out: &str, p: &str, rect: &str, radii: &str) -> String {
    format!(
        "MUL TEMP[1].xy, {rect}.zwzw, IMM[0].xxxx\n\
         ADD TEMP[1].zw, {rect}.xyxy, TEMP[1].xyxy\n\
         ADD TEMP[2].xy, {p}.xyxy, -TEMP[1].zwzw\n\
         CMP TEMP[2].z, TEMP[2].xxxx, {radii}.xxxx, {radii}.yyyy\n\
         CMP TEMP[2].w, TEMP[2].xxxx, {radii}.wwww, {radii}.zzzz\n\
         CMP TEMP[3].x, TEMP[2].yyyy, TEMP[2].zzzz, TEMP[2].wwww\n\
         MIN TEMP[3].y, TEMP[1].xxxx, TEMP[1].yyyy\n\
         MIN TEMP[3].x, TEMP[3].xxxx, TEMP[3].yyyy\n\
         ADD TEMP[3].zw, |TEMP[2].xyxy|, -TEMP[1].xyxy\n\
         ADD TEMP[3].zw, TEMP[3].zwzw, TEMP[3].xxxx\n\
         MAX TEMP[4].xy, TEMP[3].zwzw, IMM[0].zzzz\n\
         DP2 TEMP[4].z, TEMP[4].xyxy, TEMP[4].xyxy\n\
         SQRT TEMP[4].z, TEMP[4].zzzz\n\
         MAX TEMP[4].w, TEMP[3].zzzz, TEMP[3].wwww\n\
         MIN TEMP[4].w, TEMP[4].wwww, IMM[0].zzzz\n\
         ADD TEMP[4].z, TEMP[4].zzzz, TEMP[4].wwww\n\
         ADD {out}, TEMP[4].zzzz, -TEMP[3].xxxx\n"
    )
}

/// Вершинный шейдер UI: пиксели → NDC, проброс полей инстанса.
///
/// Константы: `CONST[0] = (2/w, -2/h, -1, 1)`.
pub fn ui_vertex() -> String {
    let mut s = String::from("VERT\n");
    for i in 0..UI_ATTRS {
        s += &format!("DCL IN[{i}]\n");
    }
    s += "DCL OUT[0], POSITION\n";
    for i in 0..UI_ATTRS {
        s += &format!("DCL OUT[{}], GENERIC[{}]\n", i + 1, i);
    }
    s += "DCL CONST[0]\nDCL TEMP[0]\nIMM[0] FLT32 { 0.0, 1.0, 0.0, 0.0 }\n";
    s += "MAD TEMP[0].xy, IN[0].xyxy, CONST[0].xyxy, CONST[0].zwzw\n";
    s += "MOV TEMP[0].z, IMM[0].xxxx\nMOV TEMP[0].w, IMM[0].yyyy\nMOV OUT[0], TEMP[0]\n";
    // GENERIC[0..11] = поля инстанса, GENERIC[12] = позиция пикселя.
    for i in 1..UI_ATTRS {
        s += &format!("MOV OUT[{i}], IN[{i}]\n");
    }
    s += &format!("MOV OUT[{}], IN[0]\n", UI_ATTRS);
    s += "END\n";
    s
}

/// Фрагментный uber-шейдер UI (см. таблицу видов в описании модуля).
///
/// Входы: `IN[0]` rect, `IN[1]` radii, `IN[2..4]` color0..2, `IN[5]` border,
/// `IN[6]` params, `IN[7]` grad0, `IN[8]` grad1, `IN[9]` clip, `IN[10]` extra,
/// `IN[11]` uv, `IN[12]` позиция пикселя.
/// Сэмплеры: 0 — атлас глифов, 1 — спрайты, 2 — размытый фон, 3 — обои.
/// Константы: `CONST[0] = (1/w, 1/h, 1/w8, 1/h8)`.
pub fn ui_fragment() -> String {
    let mut s = String::from("FRAG\n");
    for i in 0..UI_ATTRS {
        s += &format!("DCL IN[{i}], GENERIC[{i}], PERSPECTIVE\n");
    }
    s += "DCL OUT[0], COLOR\n";
    for i in 0..4 {
        s += &format!("DCL SAMP[{i}]\nDCL SVIEW[{i}], 2D, FLOAT\n");
    }
    s += "DCL CONST[0]\nDCL TEMP[0..21]\n";
    s += "IMM[0] FLT32 { 0.5, 1.0, 0.0, 0.0001 }\n";
    s += "IMM[1] FLT32 { 0.278393, 0.230389, 0.078108, 0.70710678 }\n";
    s += "IMM[2] FLT32 { 12.9898, 78.233, 43758.5453, 0.25 }\n";
    s += "IMM[3] FLT32 { 0.2126, 0.7152, 0.0722, 0.9 }\n";
    s += "IMM[4] FLT32 { 2.0, 3.0, 4.0, 5.0 }\n";
    s += "IMM[5] FLT32 { 6.0, 0.75, -0.75, 0.0 }\n";

    // Позиция и предумноженные цвета.
    s += "MOV TEMP[0], IN[12]\n";
    for (t, i) in [(7, 2), (8, 3), (9, 4)] {
        s += &format!("MUL TEMP[{t}].xyz, IN[{i}].xyzz, IN[{i}].wwww\nMOV TEMP[{t}].w, IN[{i}].wwww\n");
    }
    // Четыре SDF: основная фигура, фигура со сдвигом (кромка), элемент тени, отсечение.
    s += &sdf("TEMP[5].x", "TEMP[0]", "IN[0]", "IN[1]");
    s += "MOV TEMP[6], TEMP[0]\nADD TEMP[6].y, TEMP[0].yyyy, -IN[10].wwww\n";
    s += &sdf("TEMP[5].y", "TEMP[6]", "IN[0]", "IN[1]");
    s += &sdf("TEMP[5].z", "TEMP[0]", "IN[7]", "IN[8]");
    s += "MOV TEMP[16].xy, IN[9].xyxy\nADD TEMP[16].zw, IN[9].zwzw, -IN[9].xyxy\nMOV TEMP[17], IN[10].xxxx\n";
    s += &sdf("TEMP[5].w", "TEMP[0]", "TEMP[16]", "TEMP[17]");
    // Покрытия: x — фигура, y — сдвинутая фигура, z — элемент, w — отсечение.
    s += "ADD_SAT TEMP[6], IMM[0].xxxx, -TEMP[5]\n";

    // Заливка (градиент из трёх точек) → TEMP[13].
    s += "ADD TEMP[10].xy, IN[8].xyxy, -IN[7].yzyz\n\
          ADD TEMP[10].zw, TEMP[0].xyxy, -IN[7].yzyz\n\
          DP2 TEMP[11].x, TEMP[10].zwzw, TEMP[10].xyxy\n\
          DP2 TEMP[11].y, TEMP[10].xyxy, TEMP[10].xyxy\n\
          MAX TEMP[11].y, TEMP[11].yyyy, IMM[0].wwww\n\
          RCP TEMP[11].y, TEMP[11].yyyy\n\
          MUL TEMP[11].x, TEMP[11].xxxx, TEMP[11].yyyy\n\
          MAX TEMP[12].xy, IN[8].xyxy, IMM[0].wwww\n\
          RCP TEMP[12].z, TEMP[12].xxxx\n\
          RCP TEMP[12].w, TEMP[12].yyyy\n\
          MUL TEMP[12].xy, TEMP[10].zwzw, TEMP[12].zwzw\n\
          DP2 TEMP[11].z, TEMP[12].xyxy, TEMP[12].xyxy\n\
          SQRT TEMP[11].z, TEMP[11].zzzz\n\
          SEQ TEMP[11].w, IN[7].xxxx, IMM[0].yyyy\n\
          LRP TEMP[11].x, TEMP[11].wwww, TEMP[11].xxxx, TEMP[11].zzzz\n\
          MAX TEMP[11].x, TEMP[11].xxxx, IMM[0].zzzz\n\
          MAX TEMP[11].y, IN[7].wwww, IMM[0].wwww\n\
          RCP TEMP[11].y, TEMP[11].yyyy\n\
          MUL_SAT TEMP[11].y, TEMP[11].xxxx, TEMP[11].yyyy\n\
          ADD TEMP[11].z, IN[8].zzzz, -IN[7].wwww\n\
          MAX TEMP[11].z, TEMP[11].zzzz, IMM[0].wwww\n\
          RCP TEMP[11].z, TEMP[11].zzzz\n\
          ADD TEMP[11].w, TEMP[11].xxxx, -IN[7].wwww\n\
          MUL_SAT TEMP[11].z, TEMP[11].wwww, TEMP[11].zzzz\n\
          LRP TEMP[12], TEMP[11].yyyy, TEMP[8], TEMP[7]\n\
          LRP TEMP[13], TEMP[11].zzzz, TEMP[9], TEMP[8]\n\
          SGE TEMP[11].w, TEMP[11].xxxx, IN[7].wwww\n\
          LRP TEMP[13], TEMP[11].wwww, TEMP[13], TEMP[12]\n\
          SEQ TEMP[11].w, IN[7].xxxx, IMM[0].zzzz\n\
          LRP TEMP[13], TEMP[11].wwww, TEMP[7], TEMP[13]\n";

    // 0: Rect — заливка + рамка, либо световая кромка (extra.w > 0).
    s += "MUL TEMP[15], TEMP[13], TEMP[6].xxxx\n\
          ADD TEMP[11].x, TEMP[5].xxxx, IN[6].yyyy\n\
          ADD_SAT TEMP[11].x, IMM[0].xxxx, -TEMP[11].xxxx\n\
          ADD_SAT TEMP[11].x, TEMP[6].xxxx, -TEMP[11].xxxx\n\
          MUL TEMP[12].xyz, IN[5].xyzz, IN[5].wwww\n\
          MOV TEMP[12].w, IN[5].wwww\n\
          MUL TEMP[12], TEMP[12], TEMP[11].xxxx\n\
          ADD TEMP[11].y, IMM[0].yyyy, -TEMP[12].wwww\n\
          MAD TEMP[15], TEMP[15], TEMP[11].yyyy, TEMP[12]\n\
          ADD TEMP[11].z, IMM[0].yyyy, -TEMP[6].yyyy\n\
          MUL TEMP[11].z, TEMP[11].zzzz, TEMP[6].xxxx\n\
          MUL TEMP[12], TEMP[7], TEMP[11].zzzz\n\
          SLT TEMP[11].w, IMM[0].zzzz, IN[10].wwww\n\
          LRP TEMP[15], TEMP[11].wwww, TEMP[12], TEMP[15]\n\
          SEQ TEMP[20].x, IN[6].xxxx, IMM[0].zzzz\n\
          MUL TEMP[14], TEMP[15], TEMP[20].xxxx\n";

    // 1: Shadow — erf-интеграл по X и Y, маска «вне элемента».
    s += "MAX TEMP[11].x, IN[6].wwww, IMM[0].xxxx\n\
          RCP TEMP[11].x, TEMP[11].xxxx\n\
          MUL TEMP[11].x, TEMP[11].xxxx, IMM[1].wwww\n\
          ADD TEMP[12].xy, IN[11].xyxy, IN[11].zwzw\n\
          ADD TEMP[10].x, TEMP[0].xxxx, -IN[11].xxxx\n\
          ADD TEMP[10].y, TEMP[0].xxxx, -TEMP[12].xxxx\n\
          ADD TEMP[10].z, TEMP[0].yyyy, -IN[11].yyyy\n\
          ADD TEMP[10].w, TEMP[0].yyyy, -TEMP[12].yyyy\n\
          MUL TEMP[10], TEMP[10], TEMP[11].xxxx\n\
          SSG TEMP[12], TEMP[10]\n\
          MOV TEMP[11], |TEMP[10]|\n\
          MUL TEMP[15], TEMP[11], TEMP[11]\n\
          MAD TEMP[15], TEMP[15], IMM[1].zzzz, IMM[1].yyyy\n\
          MAD TEMP[15], TEMP[15], TEMP[11], IMM[1].xxxx\n\
          MAD TEMP[15], TEMP[15], TEMP[11], IMM[0].yyyy\n\
          MUL TEMP[15], TEMP[15], TEMP[15]\n\
          MUL TEMP[15], TEMP[15], TEMP[15]\n\
          RCP TEMP[19].x, TEMP[15].xxxx\n\
          RCP TEMP[19].y, TEMP[15].yyyy\n\
          RCP TEMP[19].z, TEMP[15].zzzz\n\
          RCP TEMP[19].w, TEMP[15].wwww\n\
          ADD TEMP[19], IMM[0].yyyy, -TEMP[19]\n\
          MUL TEMP[19], TEMP[12], TEMP[19]\n\
          ADD TEMP[11].x, TEMP[19].xxxx, -TEMP[19].yyyy\n\
          ADD TEMP[11].y, TEMP[19].zzzz, -TEMP[19].wwww\n\
          MUL TEMP[11].x, TEMP[11].xxxx, TEMP[11].yyyy\n\
          MUL TEMP[11].x, TEMP[11].xxxx, IMM[2].wwww\n\
          ADD TEMP[11].y, IMM[0].yyyy, -TEMP[6].zzzz\n\
          MUL TEMP[11].x, TEMP[11].xxxx, TEMP[11].yyyy\n\
          MUL TEMP[15], TEMP[7], TEMP[11].xxxx\n\
          SEQ TEMP[20].x, IN[6].xxxx, IMM[0].yyyy\n\
          MAD TEMP[14], TEMP[15], TEMP[20].xxxx, TEMP[14]\n";

    // Текстурные координаты квада: uv0 + (p - rect.xy)/rect.zw · (uv1 - uv0).
    s += "ADD TEMP[18].xy, TEMP[0].xyxy, -IN[0].xyxy\n\
          RCP TEMP[11].x, IN[0].zzzz\n\
          RCP TEMP[11].y, IN[0].wwww\n\
          MUL TEMP[18].xy, TEMP[18].xyxy, TEMP[11].xyxy\n\
          ADD TEMP[11].zw, IN[11].zwzw, -IN[11].xyxy\n\
          MAD TEMP[18].xy, TEMP[18].xyxy, TEMP[11].zwzw, IN[11].xyxy\n\
          MOV TEMP[18].zw, IMM[0].zzzz\n";
    // 2: Glyph.
    s += "TEX TEMP[19], TEMP[18], SAMP[0], 2D\n\
          MUL TEMP[15], TEMP[7], TEMP[19].xxxx\n\
          SEQ TEMP[20].x, IN[6].xxxx, IMM[4].xxxx\n\
          MAD TEMP[14], TEMP[15], TEMP[20].xxxx, TEMP[14]\n";
    // 3: Sprite (предумноженный RGBA × прозрачность).
    s += "TEX TEMP[19], TEMP[18], SAMP[1], 2D\n\
          MUL TEMP[15], TEMP[19], IN[2].wwww\n\
          SEQ TEMP[20].x, IN[6].xxxx, IMM[4].yyyy\n\
          MAD TEMP[14], TEMP[15], TEMP[20].xxxx, TEMP[14]\n";
    // 6: Image (обои).
    s += "TEX TEMP[19], TEMP[18], SAMP[3], 2D\n\
          SEQ TEMP[20].x, IN[6].xxxx, IMM[5].xxxx\n\
          MAD TEMP[14], TEMP[19], TEMP[20].xxxx, TEMP[14]\n";
    // 4: Backdrop — 4 выборки из 1/8-копии сцены, насыщенность, форма.
    s += "MUL TEMP[18].xy, TEMP[0].xyxy, CONST[0].xyxy\n\
          MOV TEMP[18].zw, IMM[0].zzzz\n\
          MAD TEMP[21].xy, CONST[0].zwzw, IMM[5].yyyy, TEMP[18].xyxy\n\
          MOV TEMP[21].zw, IMM[0].zzzz\n\
          TEX TEMP[19], TEMP[21], SAMP[2], 2D\n\
          MAD TEMP[21].xy, CONST[0].zwzw, IMM[5].zzzz, TEMP[18].xyxy\n\
          TEX TEMP[10], TEMP[21], SAMP[2], 2D\n\
          ADD TEMP[19], TEMP[19], TEMP[10]\n\
          MAD TEMP[21].xy, CONST[0].zwzw, IMM[5].yzyz, TEMP[18].xyxy\n\
          TEX TEMP[10], TEMP[21], SAMP[2], 2D\n\
          ADD TEMP[19], TEMP[19], TEMP[10]\n\
          MAD TEMP[21].xy, CONST[0].zwzw, IMM[5].zyzy, TEMP[18].xyxy\n\
          TEX TEMP[10], TEMP[21], SAMP[2], 2D\n\
          ADD TEMP[19], TEMP[19], TEMP[10]\n\
          MUL TEMP[19], TEMP[19], IMM[2].wwww\n\
          DP3 TEMP[11].x, TEMP[19].xyzz, IMM[3].xyzz\n\
          LRP TEMP[15].xyz, IN[10].yyyy, TEMP[19].xyzz, TEMP[11].xxxx\n\
          MOV_SAT TEMP[15].xyz, TEMP[15].xyzz\n\
          MOV TEMP[15].w, IMM[0].yyyy\n\
          MUL TEMP[15], TEMP[15], TEMP[6].xxxx\n\
          MUL TEMP[15], TEMP[15], IN[2].wwww\n\
          SEQ TEMP[20].x, IN[6].xxxx, IMM[4].zzzz\n\
          MAD TEMP[14], TEMP[15], TEMP[20].xxxx, TEMP[14]\n";
    // 5: Noise — хеш от целых координат пикселя.
    s += "FLR TEMP[11].xy, TEMP[0].xyxy\n\
          DP2 TEMP[11].x, TEMP[11].xyxy, IMM[2].xyxy\n\
          SIN TEMP[11].x, TEMP[11].xxxx\n\
          MUL TEMP[11].x, TEMP[11].xxxx, IMM[2].zzzz\n\
          FRC TEMP[11].x, TEMP[11].xxxx\n\
          ADD TEMP[11].x, TEMP[11].xxxx, -IMM[0].xxxx\n\
          MUL TEMP[11].y, |TEMP[11].xxxx|, IN[6].zzzz\n\
          MUL TEMP[11].y, TEMP[11].yyyy, IMM[3].wwww\n\
          MUL TEMP[11].y, TEMP[11].yyyy, TEMP[6].xxxx\n\
          SLT TEMP[11].z, IMM[0].zzzz, TEMP[11].xxxx\n\
          MUL TEMP[15].xyz, TEMP[11].yyyy, TEMP[11].zzzz\n\
          MOV TEMP[15].w, TEMP[11].yyyy\n\
          SEQ TEMP[20].x, IN[6].xxxx, IMM[4].wwww\n\
          MAD TEMP[14], TEMP[15], TEMP[20].xxxx, TEMP[14]\n";

    // Отсечение и выход (предумноженный цвет).
    s += "MUL OUT[0], TEMP[14], TEMP[6].wwww\nEND\n";
    s
}

/// Вершинный шейдер полноэкранного прохода: `IN[0] = (ndc.x, ndc.y, u, v)`.
pub fn blit_vertex() -> String {
    String::from(
        "VERT\nDCL IN[0]\nDCL OUT[0], POSITION\nDCL OUT[1], GENERIC[0]\nDCL TEMP[0]\n\
         IMM[0] FLT32 { 0.0, 1.0, 0.0, 0.0 }\n\
         MOV TEMP[0].xy, IN[0].xyxy\nMOV TEMP[0].z, IMM[0].xxxx\nMOV TEMP[0].w, IMM[0].yyyy\n\
         MOV OUT[0], TEMP[0]\nMOV OUT[1], IN[0].zwzw\nEND\n",
    )
}

/// Фрагментный шейдер уменьшения в 2 раза с фильтром 4×4 (пирамида блюра).
///
/// Константы: `CONST[0] = (1/src_w, 1/src_h, 0, 0)`.
pub fn downsample_fragment() -> String {
    String::from(
        "FRAG\nDCL IN[0], GENERIC[0], PERSPECTIVE\nDCL OUT[0], COLOR\nDCL SAMP[0]\nDCL SVIEW[0], 2D, FLOAT\n\
         DCL CONST[0]\nDCL TEMP[0..2]\n\
         IMM[0] FLT32 { 0.25, 0.0, 1.0, -1.0 }\n\
         MAD TEMP[0].xy, CONST[0].xyxy, IMM[0].wwww, IN[0].xyxy\nMOV TEMP[0].zw, IMM[0].yyyy\n\
         TEX TEMP[1], TEMP[0], SAMP[0], 2D\n\
         MAD TEMP[0].xy, CONST[0].xyxy, IMM[0].zwzw, IN[0].xyxy\nTEX TEMP[2], TEMP[0], SAMP[0], 2D\nADD TEMP[1], TEMP[1], TEMP[2]\n\
         MAD TEMP[0].xy, CONST[0].xyxy, IMM[0].wzwz, IN[0].xyxy\nTEX TEMP[2], TEMP[0], SAMP[0], 2D\nADD TEMP[1], TEMP[1], TEMP[2]\n\
         MAD TEMP[0].xy, CONST[0].xyxy, IMM[0].zzzz, IN[0].xyxy\nTEX TEMP[2], TEMP[0], SAMP[0], 2D\nADD TEMP[1], TEMP[1], TEMP[2]\n\
         MUL OUT[0], TEMP[1], IMM[0].xxxx\nEND\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Простая проверка синтаксиса: каждая строка — объявление, константа
    /// или инструкция из разрешённого набора; регистры TEMP не выходят за DCL.
    fn lint(src: &str, max_temp: u32) {
        const OPS: &[&str] = &[
            "MOV", "MOV_SAT", "ADD", "ADD_SAT", "MUL", "MUL_SAT", "MAD", "DP2", "DP3", "MIN", "MAX", "CMP", "SQRT", "RCP",
            "SEQ", "SGE", "SLT", "LRP", "SSG", "TEX", "FLR", "FRC", "SIN", "END",
        ];
        for line in src.lines() {
            let l = line.trim();
            if l.is_empty() || l == "VERT" || l == "FRAG" || l.starts_with("DCL ") || l.starts_with("IMM[") {
                continue;
            }
            let op = l.split_whitespace().next().unwrap();
            assert!(OPS.contains(&op), "неизвестная инструкция: {l}");
            for part in l.split("TEMP[").skip(1) {
                let n: u32 = part.split(']').next().unwrap().parse().unwrap();
                assert!(n <= max_temp, "TEMP[{n}] вне объявления: {l}");
            }
        }
        assert!(src.trim_end().ends_with("END"));
    }

    #[test]
    fn shaders_are_well_formed() {
        lint(&ui_vertex(), 0);
        lint(&ui_fragment(), 21);
        lint(&blit_vertex(), 0);
        lint(&downsample_fragment(), 2);
    }

    #[test]
    fn ui_fragment_reads_all_varyings() {
        let fs = ui_fragment();
        for i in 0..UI_ATTRS {
            assert!(fs.contains(&format!("IN[{i}]")), "вход IN[{i}] не используется");
        }
    }
}
