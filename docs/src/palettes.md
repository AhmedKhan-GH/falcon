# Colour palettes

Every item's `palettes` field lists its dominant colours with each colour's share of the image:

```json
"palettes": [{"color":[106,147,68],"ratio":45},{"color":[5,67,121],"ratio":31},{"color":[160,181,138],"ratio":2.22}]
```

Eagle uses these for colour search and for the swatches in the inspector.

## What Eagle does

### Which image it analyses (observed)

Eagle doesn't analyse the original file directly. It uses:

- the item's thumbnail, normally;
- the original file, when the item has `noThumbnail` but not `removeThumbnail` (small PNG, JPG, BMP and JXL files);
- a temporary thumbnail for small SVGs (`removeThumbnail`), deleted once the palette is stored.

The image is drawn onto a canvas at most **360 px wide**. It is never enlarged, and the aspect ratio is kept.

### Sampling (observed)

Eagle doesn't count every pixel of larger images. It takes every Nth pixel, with N chosen from the size of the RGBA buffer:

| Buffer size | Every Nth pixel |
|---|---|
| under 100 kB | 1 |
| under 1 MB | 10 |
| under 10 MB | 100 |
| under 100 MB | 1000 |
| larger | 10000 |

A 320 × 320 thumbnail is 409,600 bytes, so Eagle samples every 10th pixel. Pixels with alpha below 170 are skipped.

### Median cut (observed)

The colours come from **modified median cut quantization** (MMCQ), the algorithm behind the color-thief library. It is not k-means.

1. **Histogram.** Each sampled pixel is reduced to 5 bits per channel, giving 32 × 32 × 32 bins, and counted.
2. **One box.** A box is drawn around all the colours that occur.
3. **Split by population.** The box with the most pixels is cut along its longest side near the pixel median. This repeats until there are 9 boxes (75% of 12), or 5 cuts have been made.
4. **Split by population × volume.** The boxes are re-ranked by pixel count times box volume, so large sparse regions also get cut, for up to 5 more cuts.
5. **Colour of a box.** Each box's colour is the pixel-weighted average of its bins' centres.

The 5-cut limit on each pass means a palette rarely has more than about 11 colours, even though 12 are requested.

### From boxes to ratios (observed)

- `ratio` = the box's pixel count divided by the number of sampled pixels × 100, rounded to two decimals.
- A ratio of 5 or more is **truncated** to a whole number. That is why the example mixes `45` with `2.22`, and why the ratios don't add up to exactly 100.
- Colours below 0.25 are dropped, and the rest are sorted from largest to smallest.
- The top 5 are always kept. The 6th needs a ratio above 0.1, and later ones above 0.3.

### A quirk (observed)

The code that finds the starting box tracks each channel's minimum and maximum with `if … else if`. A value that lowers the minimum is never also checked against the maximum. For the first pixel, every channel lowers the starting minimum of 31 (unless it is 31 itself), so the first pixel never sets a maximum. An image with a single counted pixel therefore gets an empty box and no palette. For real images this almost never matters.

## How falcon does it

`falcon_library::palette::extract(&DynamicImage)` reproduces the whole pipeline: the 360 px analysis image, the sampling step, MMCQ with Eagle's control flow, and the rounding and filtering rules.

The port keeps the details that affect the output:

- **The bounds quirk.** It is reproduced exactly.
- **Box ordering.** Boxes live in a queue that is sorted only when popped, and only if something was pushed since the last sort. Ties keep their insertion order, so the colour order stays the same.
- **Number format.** Ratios are written as JavaScript would print them, `3` and not `3.0`, so the JSON stays byte-identical.

### Checked against Eagle

The `matches_eagle_on_fixture_thumbnails` test recomputes the palette for all 19 fixture items that have a thumbnail. Eagle computed its palettes from those same files.

| Thumbnail width | Items | Result |
|---|---|---|
| 360 px or less (no resize) | 12 | **Identical** to Eagle: every colour and every ratio |
| over 360 px (resized first) | 7 | Same dominant colour and ratio. Smaller colours differ slightly |

The quantizer is therefore an exact match. The remaining difference is the resize step. Chromium scales canvases in Skia with fixed-point arithmetic. Falcon uses one bilinear sample per pixel. Of six filters tried, point-sampling filters came closest and area-averaging filters (box, triangle) did worst, which fits Chromium sampling without mipmaps. Matching Skia bit for bit isn't worth it here.
