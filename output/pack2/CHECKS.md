# Generated texture checks

Resolution: 1024 x 1024. All 40 PNGs reopened and checked.

## Normal maps

Seam difference is the mean absolute RGB change between adjacent pixels at the join, in 8-bit units. Interior is the mean over ordinary neighboring pixels. An intentional mortar joint can exceed the global mean; the internal scanline range and the tiled height conversion provide additional context.

| Map | Mean RGB | Max length error | X seam / interior | Y seam / interior | 2x2 conversion max LSB |
|---|---|---:|---:|---:|---:|
| CircuitTraces | 127.89, 127.88, 252.75 | 0.00523 | 0.000 / 1.093 | 0.000 / 1.291 | 0 |
| SciFiHullPanels | 127.96, 127.95, 253.14 | 0.00572 | 0.383 / 0.430 | 0.186 / 0.435 | 0 |
| ParallelRidges | 127.54, 128.00, 250.83 | 0.00492 | 3.000 / 2.419 | 0.000 / 0.000 | 0 |
| CrosshatchGrooves | 127.52, 127.53, 251.01 | 0.00560 | 4.133 / 3.830 | 3.265 / 3.831 | 0 |
| CableChannels | 127.82, 127.83, 252.23 | 0.00590 | 0.000 / 0.887 | 0.000 / 0.704 | 0 |
| VentSlats | 127.87, 127.67, 252.66 | 0.00541 | 0.000 / 0.119 | 0.000 / 2.081 | 0 |
| HoneycombVent | 127.71, 127.80, 252.01 | 0.00529 | 1.688 / 2.499 | 4.240 / 2.419 | 0 |
| PerforatedMetal | 127.82, 127.82, 252.86 | 0.00597 | 0.736 / 1.000 | 0.240 / 1.001 | 0 |
| RivetedPanels | 127.95, 127.93, 253.24 | 0.00578 | 0.582 / 0.681 | 1.613 / 0.877 | 0 |
| BoltedArmor | 127.95, 127.94, 252.74 | 0.00615 | 0.000 / 0.525 | 0.000 / 0.562 | 0 |
| CoolingFins | 127.82, 127.99, 251.76 | 0.00442 | 4.224 / 3.332 | 0.000 / 0.196 | 0 |
| ConcentricMachining | 127.65, 127.65, 250.78 | 0.00591 | 5.059 / 3.976 | 4.960 / 3.976 | 0 |
| KnurledMetal | 127.50, 127.50, 250.28 | 0.00541 | 2.586 / 2.275 | 2.078 / 2.275 | 0 |
| GrateLattice | 127.93, 127.95, 252.67 | 0.00480 | 0.000 / 0.939 | 0.000 / 0.852 | 0 |
| CableBundle | 127.56, 127.79, 251.92 | 0.00505 | 0.333 / 1.550 | 0.416 / 0.378 | 0 |
| SciFiFloorPanels | 127.89, 127.89, 252.25 | 0.00615 | 0.000 / 2.564 | 2.969 / 2.453 | 0 |
| RibbedRubber | 127.51, 127.50, 252.10 | 0.00529 | 1.812 / 1.407 | 0.251 / 0.218 | 0 |
| TechGreebles | 127.80, 127.76, 252.11 | 0.00584 | 0.000 / 0.854 | 0.000 / 0.990 | 0 |
| RockStrata | 127.49, 127.52, 250.59 | 0.00584 | 0.594 / 0.621 | 1.205 / 1.224 | 0 |
| CrateredSurface | 127.54, 127.56, 251.20 | 0.00627 | 1.353 / 1.081 | 1.494 / 1.085 | 0 |

Every pixel has blue >= red and green. Flat conversion is exactly RGB (128,128,255). Green sign passes an analytic Y ramp check. Unit length deviations are from 8-bit quantization.

## Alpha maps

| Map | Range | Border maximum | Unique gray levels | Coverage |
|---|---|---:|---:|---:|
| StraightLine | 0..1 | 0 | 102 | 1.9% |
| ParallelLines | 0..1 | 0 | 144 | 4.6% |
| DashedLine | 0..1 | 0 | 243 | 1.6% |
| DottedLine | 0..1 | 0 | 235 | 0.5% |
| Crosshatch | 0..1 | 0 | 240 | 10.8% |
| GridLines | 0..1 | 0 | 14 | 8.3% |
| ConcentricRings | 0..1 | 0 | 256 | 7.5% |
| ChevronBands | 0..1 | 0 | 39 | 5.0% |
| CircuitBranch | 0..1 | 0 | 256 | 3.7% |
| CircuitNodes | 0..1 | 0 | 256 | 5.2% |
| PanelSeam | 0..1 | 0 | 42 | 1.5% |
| PanelCorner | 0..1 | 0 | 192 | 3.0% |
| BoltCluster | 0..1 | 0 | 256 | 8.6% |
| VentSlots | 0..1 | 0 | 248 | 12.1% |
| HexCellCluster | 0..1 | 0 | 40 | 3.9% |
| SciFiGlyphs | 0..1 | 0 | 47 | 2.9% |
| HUDReticle | 0..1 | 0 | 256 | 3.4% |
| TechGreebles | 0..1 | 0 | 179 | 13.4% |
| CableArc | 0..1 | 0 | 55 | 5.7% |
| EnergyArc | 0..1 | 0 | 232 | 6.4% |

Sub-LSB deterministic dither breaks up smooth 8-bit contour bands. All alphas remain single-channel, 8-bit L PNGs with an exact-black 3.5% perimeter margin.

SHA-256 file fingerprints and full seam statistics are recorded in validation.json.
