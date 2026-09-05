<!-- Written by scripts/bench_compare.py from a real run. -->

# Benchmarks

Balaur `dcc9baba4` and Godot 4.7-stable (official) on Apple M1, 8 cores, Darwin 25.6.0, 2026-09-05: the scenes of the [godot-rapier benchmark suite](https://github.com/Ughuuu/benchmarks-repo) ([post](https://godot.rapier.rs/blog/v0-35-0), [docs](https://godot.rapier.rs/docs/documentation/performance)), body for body, 300 timed steps at 60 Hz after a settle. Median physics tick in milliseconds, lower is better.

## 3D

![3D chart](/img/benchmarks/chart_3d.svg)

| | Balaur | Godot Rapier 3D | Godot Jolt | Godot Box3D | Godot Physics 3D |
| --- | ---: | ---: | ---: | ---: | ---: |
| **pyramid**<br />3795 bodies<br />![pyramid](/img/benchmarks/3d_pyramid.png) | **10.33 ms** | 14.62 ms | 26.92 ms | 13.52 ms | 48.03 ms |
| **mixed_pile**<br />5000 bodies<br />![mixed_pile](/img/benchmarks/3d_mixed_pile.png) | **4.94 ms** | 11.28 ms | 13.70 ms | 9.72 ms | 84.90 ms |
| **joint_grid**<br />5000 bodies, 9950 joints<br />![joint_grid](/img/benchmarks/3d_joint_grid.png) | **3.13 ms** | 8.92 ms | — | 10.27 ms | 26.08 ms |
| **smash**<br />5001 bodies<br />![smash](/img/benchmarks/3d_smash.png) | **8.17 ms** | 19.14 ms | 12.12 ms | 9.62 ms | 99.98 ms |
| **query_storm**<br />2000 bodies<br />![query_storm](/img/benchmarks/3d_query_storm.png) | **4.90 ms** | 5.93 ms | 26.45 ms | 6.67 ms | 73.26 ms |
| **drop**<br />8000 bodies<br />![drop](/img/benchmarks/3d_drop.png) | **3.21 ms** | 12.26 ms | 22.14 ms | 11.34 ms | — |

## 2D

![2D chart](/img/benchmarks/chart_2d.svg)

| | Balaur | Godot Rapier 2D | Godot Box2D v3 | Godot Physics 2D |
| --- | ---: | ---: | ---: | ---: |
| **pyramid**<br />10000 bodies<br />![pyramid](/img/benchmarks/2d_pyramid.png) | **7.02 ms** | 20.03 ms | 21.33 ms | 147.26 ms |
| **mixed_pile**<br />5000 bodies<br />![mixed_pile](/img/benchmarks/2d_mixed_pile.png) | **2.88 ms** | 9.25 ms | 6.71 ms | 71.12 ms |
| **joint_grid**<br />5000 bodies, 9950 joints<br />![joint_grid](/img/benchmarks/2d_joint_grid.png) | **2.20 ms** | 10.16 ms | 4.93 ms | — |
| **smash**<br />5001 bodies<br />![smash](/img/benchmarks/2d_smash.png) | **3.49 ms** | 11.86 ms | 11.57 ms | 55.09 ms |
| **query_storm**<br />2000 bodies<br />![query_storm](/img/benchmarks/2d_query_storm.png) | **4.50 ms** | 6.25 ms | 4.56 ms | 57.24 ms |
| **drop**<br />8000 bodies<br />![drop](/img/benchmarks/2d_drop.png) | **6.35 ms** | 16.54 ms | 22.66 ms | 103.34 ms |

## Nodes

Milliseconds for the whole loop, lower is better. Godot's numbers are its own published run, on a 12th-gen i5.

| operation | Balaur | Godot |
| --- | ---: | ---: |
| `add_children` | 46.12 ms | 28.19 ms |
| `delete_children_in_order` | 43.89 ms | 9.82 ms |
| `delete_children_reverse` | 44.49 ms | 6.10 ms |
| `delete_children_random` | 48.00 ms | 19.63 ms |
| `get_node` | 68.59 ms | 29.60 ms |

Balaur is the quickest engine on all 12 physics cases.

## Running it

```bash
cargo build --release -p balaur_cli --features window --bin balaur
python3 scripts/bench_compare.py --shots --godot-results <benchmarks-repo>/results
```

One case on its own, or in the editor:

```bash
target/release/balaur run examples/benchmark --headless --fixed-tick -- --case=3d/pyramid
target/release/balaur edit examples/benchmark
```

