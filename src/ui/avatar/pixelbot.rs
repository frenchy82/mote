//! DiceBear's Pixelbot (https://www.dicebear.com), CC0 1.0: the cells
//! each eye and mouth lights, as (column, row) on its 12 by 12 grid,
//! and its colors. Written by tools/gen-pixelbot.py; don't edit.

pub(super) const BACKGROUNDS: &[u32] = &[0x0f172a, 0x111827, 0x172554, 0x1e1b4b, 0x2e1065, 0x3b0764, 0x500724, 0x022c22, 0x042f2e, 0x083344];

pub(super) const GLOWS: &[u32] = &[0x7dd3fc, 0x5eead4, 0x86efac, 0xbef264, 0xfde68a, 0xfdba74, 0xf9a8d4, 0xc4b5fd, 0xffffff];

pub(super) const EYES: &[&[(u8, u8)]] = &[
    // round
    &[(3, 3), (8, 3), (2, 4), (3, 4), (4, 4), (7, 4), (8, 4), (9, 4), (3, 5), (8, 5)],
    // happy
    &[(3, 4), (8, 4), (2, 5), (4, 5), (7, 5), (9, 5)],
    // blocks
    &[(2, 4), (3, 4), (8, 4), (9, 4), (2, 5), (3, 5), (8, 5), (9, 5)],
    // soft
    &[(2, 5), (3, 5), (4, 5), (7, 5), (8, 5), (9, 5)],
    // tall
    &[(2, 3), (3, 3), (8, 3), (9, 3), (2, 4), (3, 4), (8, 4), (9, 4), (2, 5), (3, 5), (8, 5), (9, 5)],
    // slim
    &[(3, 4), (8, 4), (3, 5), (8, 5)],
    // wide
    &[(2, 4), (3, 4), (4, 4), (7, 4), (8, 4), (9, 4), (2, 5), (3, 5), (4, 5), (7, 5), (8, 5), (9, 5)],
];

pub(super) const MOUTHS: &[&[(u8, u8)]] = &[
    // smile
    &[(4, 8), (7, 8), (5, 9), (6, 9)],
    // bigSmile
    &[(3, 8), (8, 8), (4, 9), (5, 9), (6, 9), (7, 9)],
    // flat
    &[(4, 9), (5, 9), (6, 9), (7, 9)],
    // tiny
    &[(5, 9), (6, 9)],
    // open
    &[(5, 8), (6, 8), (5, 9), (6, 9)],
];
