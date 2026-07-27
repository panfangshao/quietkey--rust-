// quietkey 的图标：深蓝圆底 + 两根白色竖条（暂停符号）。
//
// 这里是图标的**唯一来源**：托盘图标、窗口图标、以及 build.rs 编进 .exe
// 资源段的那份 .ico 都从这里生成，改一处三处同步，不会出现"托盘一个样、
// 资源管理器里另一个样"。
//
// 注意：本文件会被 build.rs 用 `include!` 直接拉进构建脚本，
// 所以只能用 std，**不能引用 crate 内的任何其它模块**，也不能写内部文档注释。

/// 生成 `size`×`size` 的 RGBA8 像素（未预乘 alpha，行优先，从上到下）。
///
/// 尺寸按 32px 的原始设计等比缩放，圆边和竖条边缘都做一像素覆盖率抗锯齿，
/// 所以 16px 的托盘尺寸和 256px 的资源管理器大图标都不会糊。
pub fn rgba(size: u32) -> Vec<u8> {
    const BG: (u8, u8, u8) = (32, 96, 190);
    const FG: (u8, u8, u8) = (255, 255, 255);

    let s = size as f32;
    let center = s / 2.0;
    // 留半像素余量，避免圆的最外圈被画布裁掉。
    let radius = center - (s / 32.0).max(0.5);

    // 竖条位置沿用 32px 原图：x ∈ [9,13) ∪ [19,23)，y ∈ [9,23)
    let k = s / 32.0;
    let bars = [(9.0 * k, 13.0 * k), (19.0 * k, 23.0 * k)];
    let (bar_top, bar_bottom) = (9.0 * k, 23.0 * k);

    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let disc = (radius - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
            if disc <= 0.0 {
                continue;
            }

            let bar_x = bars
                .iter()
                .map(|&(a, b)| overlap(x as f32, a, b))
                .fold(0.0_f32, f32::max);
            let bar = bar_x * overlap(y as f32, bar_top, bar_bottom);

            let i = ((y * size + x) * 4) as usize;
            out[i] = mix(BG.0, FG.0, bar);
            out[i + 1] = mix(BG.1, FG.1, bar);
            out[i + 2] = mix(BG.2, FG.2, bar);
            out[i + 3] = (disc * 255.0).round() as u8;
        }
    }
    out
}

/// 像素 `[p, p+1)` 与区间 `[a, b)` 的重叠长度，用作抗锯齿的覆盖率。
fn overlap(p: f32, a: f32, b: f32) -> f32 {
    ((p + 1.0).min(b) - p.max(a)).clamp(0.0, 1.0)
}

fn mix(from: u8, to: u8, t: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * t).round() as u8
}

// 图标是纯计算产物，正好是这个项目里少有的能离线自测的部分：
// 尺寸缩放写错（竖条跑出圆外、小尺寸整片空白）在这里就能拦下。
#[cfg(test)]
mod tests {
    use super::rgba;

    fn px(buf: &[u8], size: u32, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let i = ((y * size + x) * 4) as usize;
        (buf[i], buf[i + 1], buf[i + 2], buf[i + 3])
    }

    #[test]
    fn buffer_len_matches_size() {
        for s in [16u32, 32, 48, 256] {
            assert_eq!(rgba(s).len(), (s * s * 4) as usize);
        }
    }

    #[test]
    fn bars_are_white_background_is_blue_corners_transparent() {
        let s = 32;
        let b = rgba(s);
        // 竖条正中：x=11 / x=21，y=16
        assert_eq!(px(&b, s, 11, 16), (255, 255, 255, 255));
        assert_eq!(px(&b, s, 21, 16), (255, 255, 255, 255));
        // 两条之间：蓝底
        assert_eq!(px(&b, s, 16, 16), (32, 96, 190, 255));
        // 圆外的四角：完全透明
        assert_eq!(px(&b, s, 0, 0).3, 0);
        assert_eq!(px(&b, s, s - 1, s - 1).3, 0);
    }

    #[test]
    fn every_size_draws_both_bars_and_a_disc() {
        for s in [16u32, 20, 24, 32, 48, 64, 128, 256] {
            let b = rgba(s);
            let opaque = (0..s * s).filter(|i| b[(i * 4 + 3) as usize] > 128).count();
            let white = (0..s * s)
                .filter(|i| b[(i * 4) as usize] > 200 && b[(i * 4 + 3) as usize] > 128)
                .count();
            // 圆面积约 πr²≈0.78·s²，给足余量只验"不是空白也没糊满"
            assert!(
                opaque > (s * s) as usize / 2 && opaque < (s * s) as usize,
                "{s}px 圆底像素数异常: {opaque}"
            );
            assert!(white >= 2, "{s}px 竖条丢失: {white}");
        }
    }
}
