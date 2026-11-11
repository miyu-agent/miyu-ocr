use super::{OVERLAP, Rect, Strip, boxes, scale, stride, strips};

#[test]
fn scale_shrinks_only_very_wide_images_and_grows_tiny_ones() {
    assert!((scale(1920, 1080) - 1.0).abs() < f32::EPSILON);
    assert!((scale(1080, 20_000) - 1.0).abs() < f32::EPSILON);
    assert!((scale(5120, 1440) - 0.5).abs() < f32::EPSILON);
    // 最短边放大到 64。
    assert!((scale(100, 20) - 3.2).abs() < 1e-6);
    assert_eq!(
        (stride(1919.6), stride(10.0), stride(1080.0)),
        (1920, 32, 1088)
    );
}

#[test]
fn a_screen_fits_in_one_strip() {
    assert_eq!(
        strips(1920, 400),
        [Strip {
            top: 0,
            bottom: 400,
            own_top: 0,
            own_bottom: 400
        }]
    );
}

/// 一张长截图：一条条叠着往下，每一行都正好算在一条里，最后一条到底。
#[test]
fn a_long_screenshot_is_cut_into_overlapping_strips() {
    let all = strips(1088, 6000);
    assert!(all.len() > 5, "{all:?}");
    for strip in &all {
        assert!(1088 * (strip.bottom - strip.top) <= 1_000_000, "{strip:?}");
        assert!(strip.own_top >= strip.top && strip.own_bottom <= strip.bottom);
    }
    for pair in all.windows(2) {
        assert_eq!(pair[1].top + OVERLAP, pair[0].bottom, "{pair:?}");
        assert_eq!(pair[0].own_bottom, pair[1].own_top, "{pair:?}");
    }
    assert_eq!((all[0].own_top, all.last().unwrap().own_bottom), (0, 6000));
}

#[test]
fn a_very_wide_image_still_gets_tall_enough_strips() {
    for strip in strips(8000, 3000) {
        assert!(
            strip.bottom - strip.top >= 256 || strip.bottom == 3000,
            "{strip:?}"
        );
    }
}

/// 两段字（两块高概率），一块概率不够，一个太小的点：只出前两段，换回原图坐标、往外扩一圈。
#[test]
fn boxes_are_the_confident_blobs_scaled_back() {
    let (w, h) = (40usize, 20usize);
    let mut map = vec![0.0f32; w * h];
    let mut fill = |x0: usize, y0: usize, x1: usize, y1: usize, p: f32| {
        for y in y0..y1 {
            for x in x0..x1 {
                map[y * w + x] = p;
            }
        }
    };
    fill(2, 2, 12, 6, 0.9);
    fill(20, 10, 36, 16, 0.8);
    fill(2, 12, 10, 16, 0.4);
    fill(30, 2, 31, 3, 0.95);
    let found = boxes(&map, w, h, 80, 40);
    assert_eq!(found.len(), 2, "{found:?}");
    // 第一块 10×4，扩出去 10×4×1.5÷(2×14) ≈ 2.14，原图是两倍大；扩出图外的裁在边上。
    let first = found[0];
    let grow: f32 = 10.0 * 4.0 * 1.5 / 28.0;
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    assert!(
        near(first.x0, ((2.0 - grow) * 2.0).max(0.0)) && near(first.y0, 0.0),
        "{first:?}"
    );
    assert!(
        near(first.x1, (12.0 + grow) * 2.0) && near(first.y1, (6.0 + grow) * 2.0),
        "{first:?}"
    );
    // 第二块贴着右边，裁在图里。
    assert!(found[1].x1 <= 80.0 && found[1].y1 <= 40.0);
}

#[test]
fn an_empty_map_has_no_boxes() {
    assert_eq!(boxes(&[0.1; 64], 8, 8, 8, 8), Vec::<Rect>::new());
}
