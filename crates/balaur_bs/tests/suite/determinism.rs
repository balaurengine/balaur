use super::{call, run};
use balaur_bs::runtime::Value;

fn rust_particles(steps: i64) -> f64 {
    let n = 10_000;
    let (mut xs, mut ys, mut vxs, mut vys) =
        (vec![0.0f64; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    for i in 0..n {
        xs[i] = (i % 100) as f64;
        ys[i] = ((i * 7) % 100) as f64;
        vxs[i] = (i % 13) as f64 - 6.0;
        vys[i] = (i % 11) as f64 - 5.0;
    }
    for _ in 0..steps {
        for i in 0..n {
            let x = xs[i] + vxs[i] * 0.016;
            let y = ys[i] + vys[i] * 0.016;
            if !(0.0..=100.0).contains(&x) {
                vxs[i] = -vxs[i];
            }
            if !(0.0..=100.0).contains(&y) {
                vys[i] = -vys[i];
            }
            xs[i] = x;
            ys[i] = y;
        }
    }
    xs.iter().sum()
}

#[test]
fn a_simulation_matches_rust_bit_for_bit() {
    let mut rt = run(r"
        pub fn particles(steps: int) -> float {
            let n = 10000;
            let xs = [0.0; n];
            let ys = [0.0; n];
            let vxs = [0.0; n];
            let vys = [0.0; n];
            for i in 0..n {
                xs[i] = i % 100;
                ys[i] = i * 7 % 100;
                vxs[i] = i % 13 - 6;
                vys[i] = i % 11 - 5;
            }
            for s in 0..steps {
                for i in 0..n {
                    let x = xs[i] + vxs[i] * 0.016;
                    let y = ys[i] + vys[i] * 0.016;
                    if x < 0.0 || x > 100.0 { vxs[i] = -vxs[i]; }
                    if y < 0.0 || y > 100.0 { vys[i] = -vys[i]; }
                    xs[i] = x;
                    ys[i] = y;
                }
            }
            let t = 0.0;
            for x in xs { t += x; }
            t
        }
    ");
    let Value::Float(got) = call(&mut rt, "particles", &[Value::Int(20)]) else {
        panic!()
    };
    assert_eq!(got.to_bits(), rust_particles(20).to_bits());
    assert_eq!(rt.live(), 0);
}

#[test]
fn math_goes_through_libm() {
    let mut rt = run(
        "pub fn f(x: float) -> float { sin(x) * cos(x) + atan2(x, 2.0) + pow(x, 1.5) + sqrt(x) }",
    );
    let x = 0.7f64;
    let want = libm::sin(x) * libm::cos(x) + libm::atan2(x, 2.0) + libm::pow(x, 1.5) + x.sqrt();
    assert_eq!(call(&mut rt, "f", &[Value::Float(x)]), Value::Float(want));
}
