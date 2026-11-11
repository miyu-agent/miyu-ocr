use rten_tensor::NdTensor;

use super::decode;

fn dictionary() -> Vec<String> {
    ["你", "好", "a"].iter().map(|s| (*s).to_string()).collect()
}

/// 每一步一个最像的类：0 是空白，1..=3 是字表，4 是空格。
fn steps(best: &[(usize, f32)], classes: usize) -> NdTensor<f32, 3> {
    let mut output = NdTensor::zeros([1, best.len(), classes]);
    for (step, &(class, p)) in best.iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "测试里几个类")]
        let rest = (1.0 - p) / (classes - 1) as f32;
        for c in 0..classes {
            output[[0, step, c]] = if c == class { p } else { rest };
        }
    }
    output
}

#[test]
fn repeats_and_blanks_collapse() {
    let output = steps(
        &[
            (1, 0.9),
            (1, 0.9),
            (0, 0.99),
            (2, 0.7),
            (4, 0.8),
            (3, 0.6),
            (0, 0.9),
            (3, 0.6),
        ],
        5,
    );
    let (text, score) = decode(&output, &dictionary());
    assert_eq!(text, "你好 aa");
    // 留下的是 你(0.9) 好(0.7) 空格(0.8) a(0.6) a(0.6)。
    assert!((score - 0.72).abs() < 1e-4, "{score}");
}

#[test]
fn all_blank_is_empty_with_no_score() {
    let (text, score) = decode(&steps(&[(0, 0.9), (0, 0.9)], 5), &dictionary());
    assert_eq!((text.as_str(), score), ("", 0.0));
}

#[test]
fn raw_scores_are_turned_into_probabilities() {
    // 没过 softmax 的输出（有负数）：照样挑最大的，分数是它的概率。
    let mut output = NdTensor::zeros([1, 1, 5]);
    for (c, v) in [-1.0f32, 3.0, 0.0, 0.0, 0.0].into_iter().enumerate() {
        output[[0, 0, c]] = v;
    }
    let (text, score) = decode(&output, &dictionary());
    assert_eq!(text, "你");
    let expected = 3f32.exp() / ((-1f32).exp() + 3f32.exp() + 3.0);
    assert!((score - expected).abs() < 1e-4, "{score} {expected}");
}
