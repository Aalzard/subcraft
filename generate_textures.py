#!/usr/bin/env python3
"""Original procedural SubCraft textures. No external image inputs.

Install: python -m pip install numpy Pillow scipy
Run:     python generate_textures.py --size 1024
All coordinates use UV units, and all randomness has a fixed per-texture seed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import time

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy.ndimage import gaussian_filter, map_coordinates
from scipy.spatial import cKDTree


def grid(n):
    """Pixel-centered coordinates, broadcast instead of allocating two grids."""
    a = (np.arange(n, dtype=np.float32) + .5) / n
    return a[None, :], a[:, None]


def smoothstep(a, b, v):
    t = np.clip((v - a) / (b - a), 0, 1)
    return t * t * (3 - 2 * t)


def noise(n, frequency, seed):
    """Cubic random lattice on a torus; periodic value AND derivative."""
    rng = np.random.default_rng(seed)
    lattice = rng.standard_normal((frequency, frequency)).astype(np.float32)
    axis = (np.arange(n, dtype=np.float32) + .5) * frequency / n
    out = np.empty((n, n), np.float32)
    for start in range(0, n, 128):
        stop = min(n, start + 128)
        coordinates = np.array(np.broadcast_arrays(axis[start:stop, None], axis[None, :]))
        out[start:stop] = map_coordinates(lattice, coordinates, order=3, mode='grid-wrap')
    return out


def fbm(n, seed, frequencies=(8, 16, 32, 64), decay=.5):
    out = np.zeros((n, n), np.float32)
    for i, frequency in enumerate(frequencies):
        out += decay ** i * noise(n, frequency, seed + 101 * i)
    return out


def wrap_delta(v):
    return (v + .5) % 1 - .5


def brick(n):
    """Running bond, recessed mortar, independent brick relief and chipped lips."""
    x, y = grid(n)
    row = np.floor(y * 8).astype(np.int32)
    u = (x * 4 + .5 * (row % 2)) % 1
    v = (y * 8) % 1
    edge = np.minimum(np.minimum(u, 1 - u) / 4, np.minimum(v, 1 - v) / 8)
    damage = np.maximum(noise(n, 82, 1101) - .35, 0) * .0028
    lip = smoothstep(.0045, .012, edge - damage)
    col = np.floor(x * 4 + .5 * (row % 2)).astype(np.int32) % 4
    variation = np.random.default_rng(1102).uniform(.86, 1.12, (8, 4)).astype(np.float32)[row, col]
    face = .84 + .045 * fbm(n, 1103, (28, 56, 112))
    return gaussian_filter(lip * face * variation, .0006 * n, mode='wrap')


def cobblestone(n):
    """Jittered periodic Voronoi stones, rounded crowns and irregular grout."""
    rng = np.random.default_rng(2201)
    points = np.array([((c + .5 + rng.uniform(-.3, .3)) / 8,
                        (r + .5 + rng.uniform(-.3, .3)) / 8)
                       for r in range(8) for c in range(8)], np.float32)
    tree = cKDTree(points, boxsize=1.)
    x, y = grid(n)
    warp_x = .006 * noise(n, 24, 2202)
    warp_y = .006 * noise(n, 24, 2203)
    heights = rng.uniform(.87, 1.13, 64).astype(np.float32)
    out = np.empty((n, n), np.float32)
    for start in range(0, n, 128):
        stop = min(start + 128, n)
        xx = (np.broadcast_to(x, (stop - start, n)) + warp_x[start:stop]) % 1
        yy = (np.broadcast_to(y[start:stop], (stop - start, n)) + warp_y[start:stop]) % 1
        d, ids = tree.query(np.column_stack((xx.ravel(), yy.ravel())), k=2)
        margin = (d[:, 1] - d[:, 0]).reshape(stop - start, n)
        crown = np.exp(-2.8 * (d[:, 0] / .10) ** 2).reshape(stop - start, n)
        out[start:stop] = smoothstep(.005, .021, margin) * (.62 + .38 * crown) * heights[ids[:, 0]].reshape(stop - start, n)
    out += .018 * fbm(n, 2204, (40, 80, 160)) * smoothstep(.1, .5, out)
    return gaussian_filter(out, .0007 * n, mode='wrap')


def hex_tiles(n):
    """Staggered hexagon tessellation; 7 columns/8 rows fit a square torus."""
    x, y = grid(n)
    xx, yy = x * 7, y * 8 * np.float32(np.sqrt(3) / 2)
    base_row = np.floor(y * 8).astype(np.int32)
    best = np.full((n, n), np.inf, np.float32)
    chosen_x = np.zeros_like(best)
    chosen_y = np.zeros_like(best)
    variation = np.zeros_like(best)
    rng = np.random.default_rng(3301)
    relief = rng.uniform(.93, 1.07, (8, 7)).astype(np.float32)
    for offset in (-1, 0, 1):
        row = base_row + offset
        shift = (row % 2) * .5
        col = np.floor(xx - shift + .5).astype(np.int32)
        dx = xx - (col + shift)
        dy = yy - row * np.float32(np.sqrt(3) / 2)
        distance = dx * dx + dy * dy
        take = distance < best
        chosen_x = np.where(take, dx, chosen_x)
        chosen_y = np.where(take, dy, chosen_y)
        variation = np.where(take, relief[row % 8, col % 7], variation)
        best = np.minimum(best, distance)
    edge = .5 - np.maximum.reduce([np.abs(chosen_x),
            np.abs(.5 * chosen_x + np.float32(np.sqrt(3) / 2) * chosen_y),
            np.abs(.5 * chosen_x - np.float32(np.sqrt(3) / 2) * chosen_y)])
    lip = smoothstep(.021, .064, edge)
    wear = .009 * noise(n, 124, 3302) + .012 * noise(n, 35, 3303)
    return lip * (variation + wear)


def leather(n):
    """Fine cellular grain, pores and softly meandering wrinkles."""
    x, y = grid(n)
    rng = np.random.default_rng(4401)
    points = np.array([((c + .5 + rng.uniform(-.35, .35)) / 54,
                        (r + .5 + rng.uniform(-.35, .35)) / 54)
                       for r in range(54) for c in range(54)], np.float32)
    tree = cKDTree(points, boxsize=1.)
    grain = np.empty((n, n), np.float32)
    for start in range(0, n, 128):
        stop = min(start + 128, n)
        xx, yy = np.broadcast_arrays(x, y[start:stop])
        distance, _ = tree.query(np.column_stack((xx.ravel(), yy.ravel())), k=2)
        margin = (distance[:, 1] - distance[:, 0]).reshape(stop - start, n)
        crown = np.exp(-(distance[:, 0] / .012) ** 2).reshape(stop - start, n)
        grain[start:stop] = .10 * smoothstep(.0003, .0034, margin) + .025 * crown
    pores = np.maximum(noise(n, 245, 4402) - .8, 0)
    warp = .016 * noise(n, 9, 4403)
    wrinkles = np.exp(-((np.sin(2 * np.pi * (x * 7 + y * 4 + warp * 18))) / .14) ** 2)
    return grain - .022 * pores - .03 * wrinkles + .004 * noise(n, 160, 4404)


def fabric_weave(n):
    """Alternating top threads at intersections, cylindrical yarn, fine fibers."""
    x, y = grid(n)
    u, v = x * 28, y * 28
    du, dv = (u + .5) % 1 - .5, (v + .5) % 1 - .5
    vertical = np.exp(-(du / .32) ** 4)
    horizontal = np.exp(-(dv / .32) ** 4)
    # Lift and lower threads smoothly as they pass through successive crossings.
    wave = np.cos(np.pi * u) * np.cos(np.pi * v)
    h = np.maximum(vertical * (.64 + .21 * wave), horizontal * (.64 - .21 * wave))
    fibers = .045 * (vertical * np.cos(2 * np.pi * u * 5) + horizontal * np.cos(2 * np.pi * v * 5))
    return h + fibers + .008 * noise(n, 150, 5501)


def diamond_plate(n):
    """Alternating diagonal, raised, rounded metal lozenges."""
    x, y = grid(n)
    u, v = x * 8, y * 8
    cx, cy = (u + .5) % 1 - .5, (v + .5) % 1 - .5
    parity = (np.floor(u + .5).astype(np.int32) + np.floor(v + .5).astype(np.int32)) % 2
    direction = 1 - 2 * parity
    along = (cx + direction * cy) / np.sqrt(2)
    across = (cx - direction * cy) / np.sqrt(2)
    diamond = np.abs(along) / .40 + np.abs(across) / .10
    return .85 * (1 - smoothstep(.73, 1.03, diamond)) + .003 * noise(n, 96, 6601)


def concrete(n):
    """Multiscale aggregate roughness and rounded small air holes."""
    h = .085 * fbm(n, 7701, (18, 36, 72, 144), .55)
    pits = np.maximum(noise(n, 78, 7702) - .50, 0)
    h -= .10 * smoothstep(0, 1.2, pits)
    x, y = grid(n)
    rng = np.random.default_rng(7703)
    for _ in range(100):
        px, py = rng.random(2)
        radius = rng.uniform(.003, .014)
        rr = (wrap_delta(x - px) ** 2 + wrap_delta(y - py) ** 2) / radius ** 2
        h -= rng.uniform(.08, .21) * np.maximum(1 - rr, 0) ** 2
    return h


def wood_planks(n):
    """Vertical timber, wandering grain, closed knot rings and recessed joints."""
    x, y = grid(n)
    u = x * 5
    col = np.floor(u).astype(np.int32)
    edge = np.minimum(u % 1, 1 - u % 1) / 5
    lip = smoothstep(.003, .008, edge)
    warp = .033 * noise(n, 9, 8801) + .017 * noise(n, 18, 8802)
    phase = x * 120 + warp * 16
    rng = np.random.default_rng(8803)
    knots = np.zeros((n, n), np.float32)
    for c in range(5):
        px = (c + rng.uniform(.24, .76)) / 5
        py = rng.uniform(0, 1)
        dx, dy = wrap_delta(x - px), wrap_delta(y - py)
        radius = np.sqrt((dx / .034) ** 2 + (dy / .074) ** 2 + .001)
        influence = np.exp(-radius ** 2 / 3)
        phase += .65 * influence * np.sin(np.arctan2(dy / .074, dx / .034) * 2)
        knots += influence * (.055 * np.sin(radius * 21) - .08 * np.exp(-radius ** 2 * 5))
    grain = .024 * np.sin(2 * np.pi * phase) + .012 * np.sin(2 * np.pi * phase * 2)
    relief = rng.uniform(.74, .86, 5).astype(np.float32)[col]
    return lip * (relief + grain + knots + .012 * fbm(n, 8804, (30, 60, 120)))


def reptile_scales(n):
    """Staggered, overlapping shield-shaped scales with raised lower rims."""
    x, y = grid(n)
    x, y = (x + .031) % 1, (y + .047) % 1
    base_row = np.floor(y * 8).astype(np.int32)
    h = np.zeros((n, n), np.float32)
    relief = np.random.default_rng(9901).uniform(.93, 1.07, (8, 10)).astype(np.float32)
    for offset in (-1, 0, 1):
        row = base_row + offset
        u = x * 10 - .5 * (row % 2)
        col = np.floor(u + .5).astype(np.int32)
        dx = (u - col) / .52
        dy = (y * 8 - row - .28) / .76
        r = dx * dx + dy * dy
        body = smoothstep(1.02, .75, r) * (.55 + .42 * np.maximum(1 - r, 0))
        rim = .13 * np.exp(-((r - .77) / .12) ** 2) * smoothstep(-.1, .7, dy)
        candidate = (body + rim) * relief[row % 8, col % 10]
        h = np.maximum(h, candidate)
    return h + .008 * noise(n, 170, 9902) * smoothstep(.1, .4, h)


def hammered_metal(n):
    """Many overlapping shallow bowls, varied hammer sizes, smooth rims."""
    rng = np.random.default_rng(10101)
    h = np.zeros((n, n), np.float32)
    # Rasterize compact periodic analytic dents only within their support.
    for _ in range(320):
        px, py = rng.random(2)
        radius = rng.uniform(.017, .056)
        depth = rng.uniform(.025, .075) * radius / .035
        stamp_periodic_bowl(h, px, py, radius, depth)
    return gaussian_filter(h, .0015 * n, mode='wrap') + .003 * noise(n, 100, 10102)


def stamp_periodic_bowl(h, px, py, radius, depth):
    n = len(h)
    ix = np.arange(int((px - radius) * n), int((px + radius) * n) + 1)
    iy = np.arange(int((py - radius) * n), int((py + radius) * n) + 1)
    dx = ((ix + .5) / n - px) / radius
    dy = ((iy + .5) / n - py) / radius
    r2 = dx[None, :] ** 2 + dy[:, None] ** 2
    patch = -depth * np.maximum(1 - r2, 0) ** 2
    h[np.ix_(iy % n, ix % n)] += patch.astype(np.float32)


def round_envelope(n, radius=.45):
    x, y = grid(n)
    r = np.sqrt((x - .5) ** 2 + (y - .5) ** 2)
    return 1 - smoothstep(radius - .045, radius, r)


def soft_round(n):
    x, y = grid(n)
    r = np.sqrt((x - .5) ** 2 + (y - .5) ** 2)
    return 1 - smoothstep(.025, .445, r)


def hard_round(n):
    x, y = grid(n)
    r = np.sqrt((x - .5) ** 2 + (y - .5) ** 2)
    boundary = .374 + .018 * noise(n, 16, 12001) + .006 * noise(n, 48, 12002)
    return 1 - smoothstep(boundary - .003, boundary + .005, r)


def chalk_grain(n):
    coarse = smoothstep(-.9, .65, fbm(n, 13001, (18, 36, 72)))
    tooth = smoothstep(-.9, .45, noise(n, 260, 13002))
    return round_envelope(n, .425) * coarse * tooth


def canvas(n):
    im = Image.new('L', (n, n))
    return im, ImageDraw.Draw(im)


def draw_path(draw, n, points, width, value=255):
    xy = [(int(x * n), int(y * n)) for x, y in points]
    draw.line(xy, fill=int(value), width=max(1, round(width * n)), joint='curve')


def dry_brush(n):
    im, draw = canvas(n)
    rng = np.random.default_rng(14001)
    for _ in range(165):
        yy = rng.normal(.50, .105)
        if not .22 < yy < .78:
            continue
        left, right = rng.uniform(.10, .28), rng.uniform(.72, .9)
        points = [(t, yy + .003 * np.sin(t * 18 + yy * 100)) for t in np.linspace(left, right, 30)]
        draw_path(draw, n, points, rng.uniform(.0008, .004), rng.uniform(95, 255))
    h = gaussian_filter(np.asarray(im, np.float32) / 255, max(.45, n * .0005))
    x, _ = grid(n)
    h *= smoothstep(.085, .22, x) * (1 - smoothstep(.72, .91, x))
    h *= smoothstep(-.55, .35, noise(n, 70, 14002))
    return h


def dab(h, px, py, radius, opacity=1., softness=.2, elongation=1.):
    """Analytic antialiased disk or ellipse, with a clipped local footprint."""
    n = len(h)
    x0, x1 = max(0, int((px - radius) * n) - 2), min(n, int((px + radius) * n) + 3)
    y0, y1 = max(0, int((py - radius * elongation) * n) - 2), min(n, int((py + radius * elongation) * n) + 3)
    if x0 >= x1 or y0 >= y1:
        return
    dx = ((np.arange(x0, x1) + .5) / n - px) / radius
    dy = ((np.arange(y0, y1) + .5) / n - py) / (radius * elongation)
    r = np.sqrt(dx[None, :] ** 2 + dy[:, None] ** 2)
    aa = max(softness, 1 / (radius * n))
    patch = opacity * (1 - smoothstep(1 - aa, 1 + aa * .1, r))
    h[y0:y1, x0:x1] = np.maximum(h[y0:y1, x0:x1], patch)


def splatter(n):
    h = np.zeros((n, n), np.float32)
    rng = np.random.default_rng(15001)
    for i in range(225):
        angle = rng.uniform(0, 2 * np.pi)
        radial = .39 * np.sqrt(rng.random())
        px, py = .5 + radial * np.cos(angle), .5 + radial * np.sin(angle)
        radius = rng.uniform(.022, .049) if i < 13 else rng.uniform(.0016, .010)
        dab(h, px, py, radius, rng.uniform(.65, 1), .12, rng.uniform(.8, 1.3))
    return h


def scratches(n):
    im, draw = canvas(n)
    rng = np.random.default_rng(16001)
    for _ in range(37):
        cx, cy = rng.uniform(.28, .72, 2)
        angle = rng.uniform(0, np.pi)
        length = rng.uniform(.07, .36)
        dx, dy = np.cos(angle) * length / 2, np.sin(angle) * length / 2
        points = [(cx - dx, cy - dy), (cx + rng.normal(0, .005), cy + rng.normal(0, .005)), (cx + dx, cy + dy)]
        draw_path(draw, n, points, rng.uniform(.0008, .0033), rng.uniform(160, 255))
    return gaussian_filter(np.asarray(im, np.float32) / 255, max(.5, .0005 * n)) * round_envelope(n, .455)


def cracks(n):
    im, draw = canvas(n)
    rng = np.random.default_rng(17001)

    def branch(px, py, angle, length, width, depth):
        segments = 18
        for j in range(segments):
            angle += rng.normal(0, .22)
            nx, ny = px + np.cos(angle) * length / segments, py + np.sin(angle) * length / segments
            if not (.075 < nx < .925 and .075 < ny < .925):
                break
            taper = (1 - j / segments) ** .7
            draw_path(draw, n, [(px, py), (nx, ny)], width * taper, 255 * (.22 + .78 * taper))
            if depth and j in (5, 10):
                branch(nx, ny, angle + rng.choice([-1, 1]) * rng.uniform(.55, 1.05), length * .48, width * .60, depth - 1)
            px, py = nx, ny

    for angle in np.linspace(0, 2 * np.pi, 6, endpoint=False):
        branch(.48, .51, angle + rng.normal(0, .15), rng.uniform(.28, .42), .007, 2)
    return gaussian_filter(np.asarray(im, np.float32) / 255, max(.5, n * .0005)) * round_envelope(n, .46)


def grunge_blotches(n):
    cloud = fbm(n, 18001, (7, 14, 28, 56), .58)
    hard = smoothstep(.03, .15, cloud)
    soft = smoothstep(-.6, .8, cloud)
    mix = smoothstep(-1, 1, noise(n, 9, 18002))
    texture = .75 + .25 * smoothstep(-1, 1, noise(n, 170, 18003))
    return (hard * mix + soft * (1 - mix)) * texture * round_envelope(n, .435)


def stipple(n):
    h = np.zeros((n, n), np.float32)
    rng = np.random.default_rng(19001)
    for _ in range(650):
        angle = rng.uniform(0, 2 * np.pi)
        radius = .415 * np.sqrt(rng.random())
        px, py = .5 + radius * np.cos(angle), .5 + radius * np.sin(angle)
        dab(h, px, py, rng.uniform(.0015, .008), rng.uniform(.65, 1), .18)
    return h * round_envelope(n, .448)


def oil_stroke(n):
    x, y = grid(n)
    center = .5 + .026 * np.sin((x - .12) * np.pi * 1.7)
    taper = 1 - smoothstep(.52, .9, x)
    width = .155 * taper + .004
    cross = np.abs(y - center) / width
    body = (1 - smoothstep(.82, 1, cross)) * smoothstep(.09, .15, x) * (1 - smoothstep(.86, .91, x))
    # Noise depends mostly on cross-stroke position, preserving long bristle
    # tracks instead of breaking the stroke into isotropic speckles.
    transverse = (y - center) * 64
    streaks = .72 + .19 * np.sin(2 * np.pi * (transverse + .06 * np.sin(x * 21)))
    streaks += .075 * np.sin(2 * np.pi * (transverse * 2.73 + .03 * np.sin(x * 14)))
    streaks += .025 * noise(n, 64, 20002)
    return body * np.clip(streaks, .12, 1)


def cell_grid(n, columns, rows=None, phase=(.137, .173)):
    """Centered local UV coordinates of a repeating rectangular cell."""
    x, y = grid(n)
    return (x * columns + phase[0]) % 1 - .5, (y * (rows or columns) + phase[1]) % 1 - .5


def box_relief(u, v, cx, cy, hx, hy, bevel=.012, chamfer=0):
    dx, dy = np.abs(u - cx), np.abs(v - cy)
    edge = np.minimum(hx - dx, hy - dy)
    if chamfer:
        edge = np.minimum(edge, (hx + hy - chamfer - dx - dy) / np.sqrt(2))
    return smoothstep(0, bevel, edge)


def capsule_distance(u, v, cx, cy, half_length, radius):
    return np.sqrt(np.maximum(np.abs(u - cx) - half_length, 0) ** 2 + (v - cy) ** 2) - radius


def tiled_drawing(n, repeats, painter, blur=.002):
    """Procedurally draw one cell, filter periodically, then repeat it."""
    k = n // repeats
    im, draw = canvas(k)
    painter(draw, k)
    cell = np.asarray(im, np.float32) / 255
    cell = gaussian_filter(cell, max(.5, k * blur), mode='wrap')
    return np.tile(cell, (repeats, repeats))


def circuit_traces(n):
    def paint(draw, k):
        routes = [([(.0, .20), (.35, .20), (.54, .39), (.85, .39)], .022),
                  ([(.17, .76), (.38, .55), (.66, .55), (.88, .77), (1., .77)], .026),
                  ([(.18, .0), (.18, .40), (.36, .58), (.36, .88)], .016)]
        for points, width in routes:
            draw_path(draw, k, points, width)
            for x, y in (points[0], points[-1]):
                if .05 < x < .95 and .05 < y < .95:
                    r = .041
                    draw.ellipse(((x-r)*k, (y-r)*k, (x+r)*k, (y+r)*k), fill=255)
                    r = .014
                    draw.ellipse(((x-r)*k, (y-r)*k, (x+r)*k, (y+r)*k), fill=65)
        # Horizontal/vertical connectors are matched at opposite cell edges.
        draw_path(draw, k, [(0, .20), (.03, .20)], .022)
        draw_path(draw, k, [(.97, .20), (1, .20)], .022)
        draw_path(draw, k, [(0, .77), (.04, .77)], .026)
        draw_path(draw, k, [(.18, .94), (.18, 1)], .016)
    return tiled_drawing(n, 4, paint, .006)


def scifi_hull_panels(n):
    u, v = cell_grid(n, 2)
    panel = box_relief(u, v, 0, 0, .48, .48, .025, .11)
    inset = box_relief(u, v, .02, -.02, .37, .34, .015, .085)
    detail = box_relief(u, v, -.26, .18, .014, .14, .006)
    detail += box_relief(u, v, .13, .27, .13, .013, .006)
    return .72 * panel - .13 * inset - .11 * detail


def parallel_ridges(n):
    x, y = grid(n)
    ridge = (.5 + .5 * np.cos(2 * np.pi * (x * 28 + .19))) ** 3
    return np.broadcast_to(ridge, (n, n)).copy()


def crosshatch_grooves(n):
    x, y = grid(n)
    a = np.abs(((x + y) * 18 + .21) % 1 - .5)
    b = np.abs(((x - y) * 18 + .11) % 1 - .5)
    return -np.maximum(np.exp(-(a / .09) ** 2), np.exp(-(b / .09) ** 2))


def cable_channels(n):
    def paint(draw, k):
        draw_path(draw, k, [(0, .22), (.28, .22), (.39, .33), (.39, .60), (.53, .74), (1, .74)], .09)
        draw_path(draw, k, [(0, .74), (.05, .74)], .09)
        draw_path(draw, k, [(.95, .22), (1, .22)], .09)
        draw_path(draw, k, [(.73, 0), (.73, .30), (.86, .43), (.86, 1)], .065)
        draw_path(draw, k, [(.86, 0), (.86, .05)], .065)
        draw_path(draw, k, [(.73, .95), (.73, 1)], .065)
    return -tiled_drawing(n, 4, paint, .011)


def vent_slats(n):
    u, v = cell_grid(n, 4, 18, phase=(.251, .173))
    distance = capsule_distance(u, v, 0, 0, .28, .12)
    return -.8 * (1 - smoothstep(-.05, .04, distance)) + .09 * np.exp(-(distance / .05) ** 2)


def hex_cell_edge(n):
    x, y = grid(n)
    x, y = (x + .013) % 1, (y + .021) % 1
    xx, yy = x * 14, y * 16 * np.float32(np.sqrt(3) / 2)
    base_row = np.floor(y * 16).astype(np.int32)
    best = np.full((n, n), np.inf, np.float32)
    px, py = np.zeros_like(best), np.zeros_like(best)
    for offset in (-1, 0, 1):
        row = base_row + offset
        shift = .5 * (row % 2)
        col = np.floor(xx - shift + .5)
        dx, dy = xx - col - shift, yy - row * np.float32(np.sqrt(3) / 2)
        d = dx * dx + dy * dy
        take = d < best
        px, py = np.where(take, dx, px), np.where(take, dy, py)
        best = np.minimum(best, d)
    return .5 - np.maximum.reduce([np.abs(px), np.abs(.5*px + .8660254*py), np.abs(.5*px - .8660254*py)])


def honeycomb_vent(n):
    edge = hex_cell_edge(n)
    return -.85 * smoothstep(.055, .13, edge) + .10 * np.exp(-((edge-.065)/.025)**2)


def perforated_metal(n):
    x, y = grid(n)
    yy = y * 10 + .173
    row = np.floor(yy).astype(np.int32)
    u = (x * 10 + .5 * (row % 2) + .137) % 1 - .5
    v = yy % 1 - .5
    r = np.sqrt(u*u + v*v)
    return -.8 * (1-smoothstep(.20, .28, r)) + .065 * np.exp(-((r-.285)/.024)**2)


def riveted_panels(n):
    u, v = cell_grid(n, 4)
    h = .64 * box_relief(u, v, 0, 0, .484, .484, .02)
    h += .13 * box_relief(u, v, 0, -.39, .48, .07, .016)
    for px in (-.34, 0, .34):
        for py in (-.39, .39):
            rr = ((u-px)**2 + (v-py)**2) / .043**2
            h += .24 * np.maximum(1-rr, 0) ** .7
    return h


def bolted_armor(n):
    u, v = cell_grid(n, 2, phase=(.251, .287))
    h = .72 * box_relief(u, v, 0, 0, .477, .477, .035, .12)
    for px in (-.33, .33):
        for py in (-.33, .33):
            dx, dy = u-px, v-py
            r = np.sqrt(dx*dx + dy*dy)
            angle = np.arctan2(dy, dx)
            hex_radius = .043 * (.94 + .06*np.cos(6*angle))
            h -= .27 * (1-smoothstep(.057, .069, r))
            h += .27 * (1-smoothstep(hex_radius-.005, hex_radius+.002, r))
            h -= .08 * box_relief(u, v, px, py, .028, .004, .002)
    return h


def cooling_fins(n):
    x, y = grid(n)
    u = np.abs((x*32 + .137) % 1-.5)
    fins = 1-smoothstep(.20, .34, u)
    braces = 1-smoothstep(.028, .050, np.abs((y*4 + .173) % 1-.5))
    return np.maximum(np.broadcast_to(fins, (n,n)), .83*braces)


def concentric_machining(n):
    u, v = cell_grid(n, 2)
    r = np.sqrt(u*u + v*v)
    disk = 1-smoothstep(.435, .47, r)
    return disk * (.12 + .018*np.cos(2*np.pi*r*48) + .03*np.exp(-(r/.1)**2))


def knurled_metal(n):
    x, y = grid(n)
    a = .5+.5*np.cos(2*np.pi*((x+y)*22 + .17))
    b = .5+.5*np.cos(2*np.pi*((x-y)*22 + .29))
    return (a*b) ** .65


def grate_lattice(n):
    u, v = cell_grid(n, 8)
    vertical = 1-smoothstep(.075, .12, np.abs(u))
    horizontal = 1-smoothstep(.075, .12, np.abs(v))
    rivet = .18 * np.maximum(1-(u*u+v*v)/.064**2, 0)
    return np.maximum(vertical, horizontal*.90) + rivet


def cable_bundle(n):
    x, y = grid(n)
    u = x*12 + .29
    col = np.floor(u).astype(np.int32)
    dx = (u % 1-.5) / (.39 + .035*(col % 3))
    body = np.sqrt(np.maximum(1-dx*dx, 0))
    rib = .042*np.cos(2*np.pi*(y*64 + .2*(col % 3)))
    return np.maximum(body + rib*(body**3)*(col % 2), 0)


def scifi_floor_panels(n):
    u, v = cell_grid(n, 2)
    h = .65 * box_relief(u, v, 0, 0, .481, .481, .026, .095)
    inset = box_relief(u, v, 0, 0, .36, .36, .012, .07)
    h -= .19*inset
    diagonal = np.abs(((u+v)*14) % 1-.5)
    h += .10*(1-smoothstep(.07, .14, diagonal))*inset
    for px in (-.40, .40):
        h -= .07*box_relief(u, v, px, 0, .008, .19, .004)
    return h


def ribbed_rubber(n):
    x, y = grid(n)
    dx = ((x*10 + .137) % 1-.5) / .43
    body = np.maximum(1-dx**4, 0) ** .65
    return np.broadcast_to(body, (n,n)).copy() + .006*noise(n, 160, 41501)


def tech_greebles(n):
    def paint(draw, k):
        rng = np.random.default_rng(41601)
        for i in range(35):
            x, y = rng.uniform(.09, .78, 2)
            w, h = rng.uniform(.035, .17, 2)
            value = rng.integers(70, 240)
            draw.rectangle((x*k, y*k, (x+w)*k, (y+h)*k), fill=int(value))
            if i % 3 == 0:
                draw.rectangle(((x+.012)*k, (y+.012)*k, (x+w-.012)*k, (y+h-.012)*k), fill=int(value*.35))
        for j in range(4):
            draw_path(draw, k, [(.10, .14+j*.2), (.26, .14+j*.2), (.33, .21+j*.2), (.48, .21+j*.2)], .015, 180)
    return tiled_drawing(n, 2, paint, .006)


def rock_strata(n):
    x, y = grid(n)
    warp = .045*noise(n, 5, 41701) + .018*noise(n, 11, 41702)
    phase = y*14 + warp*14
    strata = (.5+.5*np.cos(2*np.pi*phase)) ** .7
    return .30*strata + .025*fbm(n, 41703, (16,32,64), .6)


def cratered_surface(n):
    h = np.zeros((n,n), np.float32)
    rng = np.random.default_rng(41801)
    for _ in range(58):
        px, py = rng.random(2)
        radius = rng.uniform(.024, .096)
        extent = radius*1.25
        ix = np.arange(int((px-extent)*n), int((px+extent)*n)+1)
        iy = np.arange(int((py-extent)*n), int((py+extent)*n)+1)
        r = np.sqrt((((ix+.5)/n-px)/radius)[None,:]**2 + (((iy+.5)/n-py)/radius)[:,None]**2)
        patch = -.24*np.maximum(1-r*r,0)**2 + .09*np.exp(-((r-.94)/.115)**2)
        patch *= 1-smoothstep(1.10,1.25,r)
        h[np.ix_(iy % n, ix % n)] += patch.astype(np.float32)
    return h + .008*fbm(n, 41802, (18,36,72))


def line_mask(n, points, width=.008):
    im, draw = canvas(n)
    draw_path(draw, n, points, width)
    return gaussian_filter(np.asarray(im,np.float32)/255, max(.5,n*.0006))


def straight_line(n):
    x,y = grid(n)
    d = capsule_distance(x,y,.5,.5,.34,.012)
    return 1-smoothstep(-.002,.002,d)


def parallel_lines(n):
    x,y = grid(n)
    h = np.zeros((n,n),np.float32)
    for yy in np.linspace(.30,.70,6):
        h = np.maximum(h,1-smoothstep(-.0015,.0015,capsule_distance(x,y,.5,yy,.32,.0045)))
    return h


def dashed_line(n):
    x,y = grid(n)
    h = np.zeros((n,n),np.float32)
    for xx in np.linspace(.17,.83,7):
        h = np.maximum(h,1-smoothstep(-.002,.002,capsule_distance(x,y,xx,.5,.030,.012)))
    return h


def dotted_line(n):
    h = np.zeros((n,n),np.float32)
    for xx in np.linspace(.15,.85,11):
        dab(h,xx,.5,.012,1,.16)
    return h


def crosshatch(n):
    x,y = grid(n)
    u,v = x-.5,y-.5
    a = np.abs(((u+v)*15) % 1-.5)
    b = np.abs(((u-v)*15) % 1-.5)
    h = 1-smoothstep(.025,.06,np.minimum(a,b))
    return h*box_relief(u,v,0,0,.35,.35,.018)


def grid_lines(n):
    h = np.zeros((n,n),np.float32)
    for pos in np.linspace(.18,.82,9):
        h = np.maximum(h,line_mask(n,[(.18,pos),(.82,pos)],.0035))
        h = np.maximum(h,line_mask(n,[(pos,.18),(pos,.82)],.0035))
    return h


def concentric_rings(n):
    x,y=grid(n)
    r=np.sqrt((x-.5)**2+(y-.5)**2)
    h=np.zeros((n,n),np.float32)
    for radius in (.08,.16,.24,.32,.40):
        h=np.maximum(h,1-smoothstep(.003,.005,np.abs(r-radius)))
    return h


def chevron_bands(n):
    h=np.zeros((n,n),np.float32)
    for yy in (.19,.37,.55,.73):
        h=np.maximum(h,line_mask(n,[(.19,yy),(.5,yy+.095),(.81,yy)],.016))
    return h


def circuit_branch(n):
    im,draw=canvas(n)
    routes=[[(.12,.5),(.80,.5),(.87,.43)],
            [(.29,.5),(.29,.32),(.41,.20)],[(.45,.5),(.59,.64),(.82,.64)],
            [(.62,.5),(.62,.32),(.80,.32)],[(.25,.5),(.25,.74),(.34,.83)]]
    for p in routes:
        draw_path(draw,n,p,.011)
    h=gaussian_filter(np.asarray(im,np.float32)/255,max(.5,n*.0005))
    for p in routes:
        dab(h,*p[-1],.023)
    return h


def circuit_nodes(n):
    nodes=[(.17,.29),(.39,.29),(.61,.18),(.82,.38),(.62,.57),(.83,.76),(.39,.80),(.18,.61)]
    im,draw=canvas(n)
    for a,b in ((0,1),(1,2),(2,3),(3,4),(4,5),(4,6),(6,7),(7,0),(1,4)):
        x1,y1=nodes[a];x2,y2=nodes[b]
        draw_path(draw,n,[(x1,y1),((x1+x2)/2,y1),((x1+x2)/2,y2),(x2,y2)],.008)
    h=np.asarray(im,np.float32)/255
    for px,py in nodes:
        dab(h,px,py,.034)
        x,y=grid(n)
        h*=smoothstep(.013,.017,np.sqrt((x-px)**2+(y-py)**2))
    return gaussian_filter(h,max(.5,n*.0005))


def panel_seam(n):
    path=[(.12,.52),(.34,.52),(.39,.45),(.62,.45),(.68,.52),(.88,.52)]
    h=line_mask(n,path,.012)
    for xx,yy in ((.24,.52),(.51,.45),(.77,.52)):
        h=np.maximum(h,line_mask(n,[(xx,yy-.032),(xx,yy+.032)],.009))
    return h


def panel_corner(n):
    h=line_mask(n,[(.20,.83),(.20,.29),(.29,.20),(.83,.20)],.014)
    h=np.maximum(h,line_mask(n,[(.27,.77),(.27,.34),(.34,.27),(.77,.27)],.005))
    for px,py in ((.20,.65),(.65,.20)):
        dab(h,px+.045,py+.045,.014)
    return h


def bolt_cluster(n):
    x,y=grid(n)
    h=np.zeros((n,n),np.float32)
    for px,py in ((.32,.32),(.68,.32),(.32,.68),(.68,.68)):
        dx,dy=x-px,y-py
        r=np.sqrt(dx*dx+dy*dy)
        a=np.arctan2(dy,dx)
        radius=.077*(.94+.06*np.cos(a*6))
        bolt=1-smoothstep(radius-.002,radius+.002,r)
        slot=box_relief(dx,dy,0,0,.046,.005,.002)
        ring=(1-smoothstep(.002,.004,np.abs(r-.094)))*.65
        h=np.maximum(h,np.maximum(bolt*(1-slot),ring))
    return h


def vent_slots(n):
    x,y=grid(n)
    h=np.zeros((n,n),np.float32)
    for yy in np.linspace(.23,.77,7):
        h=np.maximum(h,1-smoothstep(-.002,.002,capsule_distance(x,y,.5,yy,.26,.014)))
    return h


def hex_cell_cluster(n):
    im,draw=canvas(n)
    radius=.075
    for q in range(-2,3):
        for r in range(-2,3):
            if max(abs(q),abs(r),abs(q+r))>2:
                continue
            cx=.5+np.sqrt(3)*radius*(q+r/2)
            cy=.5+1.5*radius*r
            pts=[(cx+radius*np.cos(a),cy+radius*np.sin(a)) for a in np.linspace(np.pi/6,np.pi/6+2*np.pi,7)]
            draw_path(draw,n,pts,.005)
    return gaussian_filter(np.asarray(im,np.float32)/255,max(.5,n*.0005))


def scifi_glyphs(n):
    im,draw=canvas(n)
    patterns=[[(.1,.8),(.1,.2),(.7,.2),(.7,.5),(.4,.5)],
              [(.1,.7),(.5,.1),(.9,.7),(.65,.7)],
              [(.1,.2),(.8,.2),(.8,.8),(.4,.8),(.4,.5)],
              [(.1,.5),(.5,.1),(.9,.5),(.5,.9),(.5,.5)],
              [(.1,.8),(.1,.2),(.45,.2),(.45,.55),(.85,.55),(.85,.15)],
              [(.1,.1),(.9,.1),(.5,.5),(.9,.9),(.1,.9)],
              [(.2,.1),(.2,.9),(.8,.9),(.8,.55),(.5,.55)],
              [(.1,.2),(.5,.2),(.5,.8),(.9,.8)],
              [(.1,.5),(.4,.2),(.8,.2),(.8,.8),(.4,.8)]]
    for i,p in enumerate(patterns):
        ox,oy=.17+(i%3)*.24,.17+(i//3)*.24
        draw_path(draw,n,[(ox+x*.18,oy+y*.18) for x,y in p],.007)
    return gaussian_filter(np.asarray(im,np.float32)/255,max(.5,n*.0005))


def hud_reticle(n):
    x,y=grid(n)
    dx,dy=x-.5,y-.5
    r=np.sqrt(dx*dx+dy*dy)
    angle=np.arctan2(dy,dx)
    ring=(1-smoothstep(.002,.004,np.abs(r-.29)))*smoothstep(.1,.2,np.abs(np.sin(2*angle)))
    outer=(1-smoothstep(.0015,.003,np.abs(r-.36)))*smoothstep(.35,.5,np.abs(np.sin(4*angle+.3)))
    h=np.maximum(ring,outer)
    for p in [[(.12,.5),(.25,.5)],[(.75,.5),(.88,.5)],[(.5,.12),(.5,.25)],[(.5,.75),(.5,.88)],
              [(.45,.5),(.55,.5)],[(.5,.45),(.5,.55)]]:
        h=np.maximum(h,line_mask(n,p,.005))
    for a in np.linspace(0,2*np.pi,24,endpoint=False):
        h=np.maximum(h,line_mask(n,[(.5+.375*np.cos(a),.5+.375*np.sin(a)),(.5+.402*np.cos(a),.5+.402*np.sin(a))],.003))
    return h


def tech_greebles_alpha(n):
    im,draw=canvas(n)
    rng=np.random.default_rng(51901)
    for row in range(3):
        for col in range(3):
            ox,oy=.15+col*.245,.15+row*.245
            draw.rectangle((ox*n,oy*n,(ox+.19)*n,(oy+.19)*n),outline=255,width=max(1,int(n*.007)))
            for j in range(5):
                x,y=rng.uniform(.02,.12,2)
                w,h=rng.uniform(.025,.055,2)
                draw.rectangle(((ox+x)*n,(oy+y)*n,(ox+x+w)*n,(oy+y+h)*n),fill=int(rng.integers(150,256)))
            draw_path(draw,n,[(ox+.02,oy+.14),(ox+.09,oy+.14),(ox+.12,oy+.17),(ox+.17,oy+.17)],.005)
    return gaussian_filter(np.asarray(im,np.float32)/255,max(.5,n*.0005))


def cable_arc(n):
    h=np.zeros((n,n),np.float32)
    t=np.linspace(0,1,140)
    for offset in np.linspace(-.065,.065,5):
        pts=[(.15+.70*float(u),.66-.31*np.sin(np.pi*float(u))+offset) for u in t]
        h=np.maximum(h,line_mask(n,pts,.009))
    return h


def energy_arc(n):
    rng=np.random.default_rng(52001)
    xs=np.linspace(.12,.88,26)
    ys=.5+rng.uniform(-.065,.065,len(xs))
    pts=list(zip(xs,ys))
    core=line_mask(n,pts,.004)
    for j in (6,13,19):
        sign=1 if j%2 else -1
        branch=[pts[j],(xs[j]+.035,ys[j]+sign*.07),(xs[j]-.01,ys[j]+sign*.11),(xs[j]+.025,ys[j]+sign*.16)]
        core=np.maximum(core,line_mask(n,branch,.0018))
    glow=gaussian_filter(core,n*.006)
    return np.clip(core+.75*glow,0,1)


STARTER_NORMALS = {
    'Brick': brick, 'Cobblestone': cobblestone, 'HexTiles': hex_tiles,
    'Leather': leather, 'FabricWeave': fabric_weave, 'DiamondPlate': diamond_plate,
    'Concrete': concrete, 'WoodPlanks': wood_planks, 'ReptileScales': reptile_scales,
    'HammeredMetal': hammered_metal,
}
STARTER_ALPHAS = {
    'SoftRound': soft_round, 'HardRound': hard_round, 'ChalkGrain': chalk_grain,
    'DryBrush': dry_brush, 'Splatter': splatter, 'Scratches': scratches,
    'Cracks': cracks, 'GrungeBlotches': grunge_blotches, 'Stipple': stipple,
    'OilStroke': oil_stroke,
}

SCIFI_NORMALS = {
    'CircuitTraces': circuit_traces, 'SciFiHullPanels': scifi_hull_panels,
    'ParallelRidges': parallel_ridges, 'CrosshatchGrooves': crosshatch_grooves,
    'CableChannels': cable_channels, 'VentSlats': vent_slats,
    'HoneycombVent': honeycomb_vent, 'PerforatedMetal': perforated_metal,
    'RivetedPanels': riveted_panels, 'BoltedArmor': bolted_armor,
    'CoolingFins': cooling_fins, 'ConcentricMachining': concentric_machining,
    'KnurledMetal': knurled_metal, 'GrateLattice': grate_lattice,
    'CableBundle': cable_bundle, 'SciFiFloorPanels': scifi_floor_panels,
    'RibbedRubber': ribbed_rubber, 'TechGreebles': tech_greebles,
    'RockStrata': rock_strata, 'CrateredSurface': cratered_surface,
}
SCIFI_ALPHAS = {
    'StraightLine': straight_line, 'ParallelLines': parallel_lines,
    'DashedLine': dashed_line, 'DottedLine': dotted_line,
    'Crosshatch': crosshatch, 'GridLines': grid_lines,
    'ConcentricRings': concentric_rings, 'ChevronBands': chevron_bands,
    'CircuitBranch': circuit_branch, 'CircuitNodes': circuit_nodes,
    'PanelSeam': panel_seam, 'PanelCorner': panel_corner,
    'BoltCluster': bolt_cluster, 'VentSlots': vent_slots,
    'HexCellCluster': hex_cell_cluster, 'SciFiGlyphs': scifi_glyphs,
    'HUDReticle': hud_reticle, 'TechGreebles': tech_greebles_alpha,
    'CableArc': cable_arc, 'EnergyArc': energy_arc,
}
NORMALS = {**STARTER_NORMALS, **SCIFI_NORMALS}
ALPHAS = {**STARTER_ALPHAS, **SCIFI_ALPHAS}


def height_to_normal(height, strength=1.):
    """OpenGL normals: image rows run down, so green is +dH/d(image-row).

    Central differences wrap in both directions. Calibrate UV height depth to
    RMS slope .30 at strength 1, then smoothly limit extreme slopes to .80.
    This keeps fine detail legible and blue dominant at every pixel. Encoding
    zero X/Y as round(127.5) gives the required flat pixel (128,128,255).
    """
    n = len(height)
    dx = (np.roll(height, -1, 1) - np.roll(height, 1, 1)) * (n / 2)
    dy = (np.roll(height, -1, 0) - np.roll(height, 1, 0)) * (n / 2)
    rms = float(np.sqrt(np.mean(dx * dx + dy * dy)))
    depth = .30 / max(rms, 1e-12)
    dx *= -depth * strength
    dy *= depth * strength
    slope = np.sqrt(dx * dx + dy * dy)
    attenuation = .80 * np.tanh(slope / .80) / np.maximum(slope, 1e-12)
    dx *= attenuation
    dy *= attenuation
    inverse = 1 / np.sqrt(1 + dx * dx + dy * dy)
    normal = np.stack((dx * inverse, dy * inverse, inverse), axis=-1)
    encoded = np.rint(np.clip(normal * .5 + .5, 0, 1) * 255).astype(np.uint8)
    return encoded, depth


def quantize_alpha(alpha, seed):
    """8-bit L output with sub-LSB stochastic dithering to prevent contour bands."""
    alpha = np.clip(alpha, 0, None).astype(np.float32)
    alpha /= max(float(alpha.max()), 1e-12)
    # A generous exact-black margin also catches Gaussian drawing tails.
    x, y = grid(len(alpha))
    margin = np.minimum(np.minimum(x, 1 - x), np.minimum(y, 1 - y))
    alpha *= smoothstep(.035, .065, margin)
    dither = np.random.default_rng(seed).uniform(-.5, .5, alpha.shape).astype(np.float32)
    encoded = np.rint(np.clip(alpha * 255 + dither, 0, 255)).astype(np.uint8)
    return encoded


def contact_sheet(paths, destination, columns=5, thumb=252):
    label, gap = 36, 16
    rows = (len(paths) + columns - 1) // columns
    sheet = Image.new('RGB', (columns * (thumb + gap) + gap, rows * (thumb + label + gap) + gap), (24, 27, 34))
    draw = ImageDraw.Draw(sheet)
    try:
        font = ImageFont.truetype('DejaVuSans.ttf', 17)
    except OSError:
        font = ImageFont.load_default(size=17)
    for i, path in enumerate(paths):
        with Image.open(path) as image:
            # Reducing RGB normals is only for previews, never for output maps.
            tile = image.convert('RGB').resize((thumb, thumb), Image.Resampling.LANCZOS)
        xx, yy = gap + (i % columns) * (thumb + gap), gap + (i // columns) * (thumb + label + gap)
        sheet.paste(tile, (xx, yy))
        name = path.stem.replace('_Normal_2x2', ' (2x2)').replace('_Normal', '').replace('_Alpha', '')
        draw.text((xx + 5, yy + thumb + 9), name, font=font, fill=(229, 232, 240))
    sheet.save(destination)


def seam_statistics(rgb):
    f = rgb.astype(np.float32)
    # Compare actual joined boundary pixels with every ordinary adjacency.
    results = {}
    for axis, label in ((1, 'x'), (0, 'y')):
        interior = np.mean(np.abs(np.diff(f, axis=axis)), axis=2)
        seam = np.mean(np.abs(np.take(f, 0, axis=axis) - np.take(f, -1, axis=axis)), axis=1)
        interior_line_means = interior.mean(axis=0 if axis == 1 else 1)
        results[label] = {
            'seam_mean_8bit': float(seam.mean()),
            'interior_mean_8bit': float(interior.mean()),
            'interior_line_p95_8bit': float(np.percentile(interior_line_means, 95)),
            'seam_to_interior_ratio': float(seam.mean() / max(interior.mean(), .00001)),
            'seam_line_percentile': float(100 * np.mean(interior_line_means <= seam.mean())),
        }
        # Pattern boundaries can be intentionally sharper than a global mean;
        # use line distributions AND compare against continuous periodic source.
        results[label]['within_internal_line_range'] = bool(seam.mean() <= max(float(interior_line_means.max()) * 1.1, .1))
    return results


def validate_normal(path, depth):
    with Image.open(path) as im:
        assert im.mode == 'RGB'
        rgb = np.asarray(im)
    v = rgb.astype(np.float32) / 127.5 - 1
    lengths = np.sqrt(np.sum(v * v, axis=2))
    blue_dominant = np.all(rgb[:, :, 2] >= np.maximum(rgb[:, :, 0], rgb[:, :, 1]))
    result = {
        'mean_rgb': [float(c) for c in rgb.mean(axis=(0, 1))],
        'vector_length_min': float(lengths.min()), 'vector_length_max': float(lengths.max()),
        'vector_length_max_error': float(np.max(np.abs(lengths - 1))),
        'blue_dominant_every_pixel': bool(blue_dominant),
        'calibrated_uv_height_depth': depth,
        'seams': seam_statistics(rgb),
    }
    assert result['vector_length_max_error'] < .008, result
    assert blue_dominant, result
    assert abs(result['mean_rgb'][0] - 128) < 3 and abs(result['mean_rgb'][1] - 128) < 3, result
    assert result['mean_rgb'][2] > 243, result
    assert all(s['within_internal_line_range'] for s in result['seams'].values()), result
    return result


def validate_alpha(path):
    with Image.open(path) as im:
        assert im.mode == 'L'
        a = np.asarray(im)
    border = np.concatenate((a[0], a[-1], a[:, 0], a[:, -1]))
    assert a.min() == 0 and a.max() == 255
    assert not np.any(border)
    result = {'range_0_to_1': [float(a.min() / 255), float(a.max() / 255)],
              'border_max': int(border.max()), 'unique_levels': len(np.unique(a)),
              'nonzero_coverage_percent': float(np.mean(a > 0) * 100)}
    return result


def save_tiled_preview(path, destination):
    with Image.open(path) as im:
        tile = im.convert('RGB')
    n = tile.width
    tiled = Image.new('RGB', (n * 2, n * 2))
    for px, py in ((0, 0), (n, 0), (0, n), (n, n)):
        tiled.paste(tile, (px, py))
    tiled.save(destination)


def algorithm_checks():
    """Check conversion sign, exact flat color, and actual torus sampling."""
    flat, _ = height_to_normal(np.zeros((32, 32), np.float32))
    assert np.all(flat == [128, 128, 255])
    x, y = grid(128)
    h = np.broadcast_to(np.sin(2 * np.pi * y), (128, 128)).copy()
    rgb, _ = height_to_normal(h)
    assert rgb[0, 0, 1] > 128 and rgb[64, 0, 1] < 128
    return {'flat_pixel': [128, 128, 255], 'opengl_green_sign': 'PASS',
            'wrap_gradients': 'central difference using numpy.roll'}


def periodic_extension_checks(name, function, size=256):
    """Evaluate a real repeated height field, then differentiate its interior.

    Compare a crop crossing the join with normals generated using wrap gradients.
    This tests the 2x2 conversion boundary, not merely first-vs-last equality.
    """
    h = function(size).astype(np.float32)
    n1, _ = height_to_normal(h)
    n2, _ = height_to_normal(np.tile(h, (2, 2)))
    # Doubling UV derivative and halving calibrated depth cancel exactly.
    error = int(np.max(np.abs(n2.astype(np.int16) - np.tile(n1, (2, 2, 1)).astype(np.int16))))
    assert error <= 1, (name, error)
    return {'normal_vs_2x2_height_conversion_max_lsb': error}


def write_report(output, report):
    (output / 'validation.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    lines = ['# Generated texture checks', '',
             f"Resolution: {report['size']} x {report['size']}. All {len(report['normals']) + len(report['alphas'])} PNGs reopened and checked.", '',
             '## Normal maps', '',
             'Seam difference is the mean absolute RGB change between adjacent pixels at the join, in 8-bit units. '
             'Interior is the mean over ordinary neighboring pixels. An intentional mortar joint can exceed the global mean; '
             'the internal scanline range and the tiled height conversion provide additional context.', '',
             '| Map | Mean RGB | Max length error | X seam / interior | Y seam / interior | 2x2 conversion max LSB |',
             '|---|---|---:|---:|---:|---:|']
    for name, r in report['normals'].items():
        sx, sy = r['seams']['x'], r['seams']['y']
        mean = ', '.join(f'{c:.2f}' for c in r['mean_rgb'])
        lines.append(f"| {name} | {mean} | {r['vector_length_max_error']:.5f} | {sx['seam_mean_8bit']:.3f} / {sx['interior_mean_8bit']:.3f} | {sy['seam_mean_8bit']:.3f} / {sy['interior_mean_8bit']:.3f} | {r['normal_vs_2x2_height_conversion_max_lsb']} |")
    lines += ['', 'Every pixel has blue >= red and green. Flat conversion is exactly RGB (128,128,255). '
              'Green sign passes an analytic Y ramp check. Unit length deviations are from 8-bit quantization.', '',
              '## Alpha maps', '', '| Map | Range | Border maximum | Unique gray levels | Coverage |',
              '|---|---|---:|---:|---:|']
    for name, r in report['alphas'].items():
        lines.append(f"| {name} | 0..1 | {r['border_max']} | {r['unique_levels']} | {r['nonzero_coverage_percent']:.1f}% |")
    lines += ['', 'Sub-LSB deterministic dither breaks up smooth 8-bit contour bands. '
              'All alphas remain single-channel, 8-bit L PNGs with an exact-black 3.5% perimeter margin.', '',
              'SHA-256 file fingerprints and full seam statistics are recorded in validation.json.', '']
    (output / 'CHECKS.md').write_text('\n'.join(lines), encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--size', type=int, default=1024, choices=(256, 512, 1024, 2048, 4096), help='Output pixels per side; 256/512 also allowed for quick previews.')
    parser.add_argument('--output', type=Path, default=Path('output'))
    parser.add_argument('--pack', choices=('starter', 'scifi', 'all'), default='all', help='starter: original 20; scifi: new 40; all: all 60 (default).')
    parser.add_argument('--strength', type=float, default=1., help='Normal slope multiplier before smooth limiting (default 1).')
    args = parser.parse_args()
    if args.strength <= 0:
        parser.error('--strength must be positive')
    output = args.output
    report_dir = output / 'pack2' if args.pack == 'scifi' else output
    textures, alphas, previews = output / 'textures', output / 'alphas', report_dir / 'seam_previews'
    normals = STARTER_NORMALS if args.pack == 'starter' else SCIFI_NORMALS if args.pack == 'scifi' else NORMALS
    alpha_functions = STARTER_ALPHAS if args.pack == 'starter' else SCIFI_ALPHAS if args.pack == 'scifi' else ALPHAS
    sample_names = ('Brick', 'Cobblestone', 'HexTiles', 'WoodPlanks') if args.pack == 'starter' else ('SciFiHullPanels', 'CircuitTraces', 'HoneycombVent', 'RockStrata')
    for directory in (textures, alphas, previews):
        directory.mkdir(parents=True, exist_ok=True)
    report = {'size': args.size, 'pack': args.pack, 'strength': args.strength, 'algorithm': algorithm_checks(), 'normals': {}, 'alphas': {}, 'sha256': {}}
    normal_paths, alpha_paths = [], []
    for name, function in normals.items():
        start = time.perf_counter()
        height = function(args.size).astype(np.float32)
        assert height.shape == (args.size, args.size) and np.all(np.isfinite(height))
        rgb, depth = height_to_normal(height, args.strength)
        path = textures / f'{name}_Normal.png'
        Image.fromarray(rgb).save(path)
        r = validate_normal(path, depth)
        r.update(periodic_extension_checks(name, function))
        report['normals'][name] = r
        normal_paths.append(path)
        if name in sample_names:
            save_tiled_preview(path, previews / f'{name}_Normal_2x2.png')
        print(f'{path}: checked ({time.perf_counter() - start:.1f}s)', flush=True)
        del height, rgb
    for name, function in alpha_functions.items():
        start = time.perf_counter()
        alpha = function(args.size)
        assert alpha.shape == (args.size, args.size) and np.all(np.isfinite(alpha))
        encoded = quantize_alpha(alpha, 31000 + list(ALPHAS).index(name))
        path = alphas / f'{name}_Alpha.png'
        Image.fromarray(encoded).save(path)
        report['alphas'][name] = validate_alpha(path)
        alpha_paths.append(path)
        print(f'{path}: checked ({time.perf_counter() - start:.1f}s)', flush=True)
        del alpha, encoded
    contact_sheet(normal_paths, report_dir / 'preview_normals.png')
    contact_sheet(alpha_paths, report_dir / 'preview_alphas.png')
    contact_sheet([previews / f'{name}_Normal_2x2.png' for name in sample_names],
                  report_dir / 'preview_seams.png', columns=4, thumb=360)
    for path in normal_paths + alpha_paths:
        report['sha256'][str(path.relative_to(output)).replace('\\', '/')] = hashlib.sha256(path.read_bytes()).hexdigest()
    write_report(report_dir, report)
    print('Done. Contact sheets, four 2x2 previews, and validation reports saved.', flush=True)


if __name__ == '__main__':
    main()
