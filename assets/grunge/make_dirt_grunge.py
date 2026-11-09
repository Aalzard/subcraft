# SubCraft grunge map generator (CC0). Usage: python make_dirt_grunge.py out.png [size] [seed]
# Seamless grayscale dirt/grunge texture by spectral synthesis (all operations periodic).
import numpy as np, sys
from PIL import Image
N = int(sys.argv[2]) if len(sys.argv) > 2 else 1024
rng = np.random.default_rng(int(sys.argv[3]) if len(sys.argv) > 3 else 7)
fy = np.fft.fftfreq(N)[:, None]; fx = np.fft.fftfreq(N)[None, :]
fr = np.sqrt(fx * fx + fy * fy); fr[0, 0] = 1.0

def norm(a):
    a = a - a.mean(); return a / (a.std() + 1e-9)

def fbm(beta, lo=0.0, hi=0.5, aniso=None):
    w = np.fft.fft2(rng.standard_normal((N, N)))
    f = fr.copy()
    if aniso is not None:
        ang, k = aniso
        u = fx * np.cos(ang) + fy * np.sin(ang); v = -fx * np.sin(ang) + fy * np.cos(ang)
        f = np.sqrt(u * u + (v * k) ** 2); f[0, 0] = 1.0
    amp = f ** (-beta / 2.0)
    amp[(fr < lo) | (fr > hi)] = 0.0
    amp[0, 0] = 0.0
    return norm(np.real(np.fft.ifft2(w * amp)))

def blur(a, sigma):
    g = np.exp(-2 * (np.pi * sigma) ** 2 * (fx * fx + fy * fy))
    return np.real(np.fft.ifft2(np.fft.fft2(a) * g))

def warp(a, dx, dy, amt):
    yy, xx = np.mgrid[0:N, 0:N].astype(np.float32)
    x = (xx + dx * amt) % N; y = (yy + dy * amt) % N
    x0 = np.floor(x).astype(int); y0 = np.floor(y).astype(int); tx = x - x0; ty = y - y0
    x1 = (x0 + 1) % N; y1 = (y0 + 1) % N
    return (a[y0, x0] * (1 - tx) + a[y0, x1] * tx) * (1 - ty) + (a[y1, x0] * (1 - tx) + a[y1, x1] * tx) * ty

def sstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1); return t * t * (3 - 2 * t)

s = N / 1024.0
# Base grime: broad patchy deposits, domain warped so blotches are irregular.
base = fbm(2.6, 1.5 / N)
wx, wy = fbm(2.2), fbm(2.2)
base = norm(warp(base, wx, wy, 14 * s))
# Stains: thresholded mid-frequency field with darker rims (dried water marks).
st_f = norm(warp(fbm(3.2, 3.0 / N), wx, wy, 10 * s))
stain = sstep(0.3, 0.9, st_f)
rim = np.clip(np.abs(np.gradient(blur(stain, 1.5 * s))[0]) + np.abs(np.gradient(blur(stain, 1.5 * s))[1]), 0, None)
rim = rim / (rim.max() + 1e-9)
# Smudges: anisotropic streaks in a couple of directions.
smudge = 0.6 * fbm(2.4, 2.0 / N, aniso=(0.4, 6.0)) + 0.4 * fbm(2.4, 2.0 / N, aniso=(2.1, 4.0))
# Grit: sparse specks of several sizes, irregular via warp.
grit = np.zeros((N, N))
for count, sig, strength in [(int(9000 * s * s), 0.6 * s, 1.0), (int(1800 * s * s), 1.4 * s, 0.8), (int(250 * s * s), 3.0 * s, 0.6)]:
    d = np.zeros((N, N)); ys = rng.integers(0, N, count); xs = rng.integers(0, N, count)
    np.add.at(d, (ys, xs), rng.lognormal(0, 0.6, count))
    d = blur(d, sig); grit += strength * d / (d.max() + 1e-9)
grit = warp(grit, fbm(1.5, 20.0 / N), fbm(1.5, 20.0 / N), 2 * s)
grit *= sstep(-0.8, 1.2, base)  # specks gather where grime is
# Fine porous grain, multiplicative (lognormal look of real dust).
grain = fbm(1.2, 60.0 / N)
pores = sstep(1.2, 2.6, fbm(1.8, 80.0 / N))

v = 0.8 * base + 0.35 * norm(blur(stain, 2*s)) + 0.22 * norm(rim) + 0.3 * smudge
v = norm(v)
v = v + 0.45 * norm(grit) + 0.22 * grain - 0.35 * pores
v = norm(v)
# Histogram: map to a natural, slightly skewed distribution (mostly mid-dark, bright dirt tail).
order = np.argsort(v, axis=None)
ranks = np.empty(N * N); ranks[order] = (np.arange(N * N) + 0.5) / (N * N)
from math import erf
# Beta-like target via power curve on ranks.
out = ranks.reshape(N, N)
out = 0.5 + 0.5 * np.tanh(2.2 * (out - 0.5)) / np.tanh(1.1)
out = out ** 1.2
out = 0.04 + 0.92 * out
Image.fromarray(np.clip(out * 255 + 0.5, 0, 255).astype(np.uint8), 'L').save(sys.argv[1], optimize=True)
t = Image.open(sys.argv[1]); big = Image.new('L', (N * 2, N * 2))
for i in range(2):
    for j in range(2): big.paste(t, (i * N, j * N))
big.save(sys.argv[1].replace('.png', '_tiled2x2.png'))
print('mean', out.mean(), 'std', out.std())
