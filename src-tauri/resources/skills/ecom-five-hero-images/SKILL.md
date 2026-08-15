---
name: ecom-five-hero-images
description: Create a five-image e-commerce hero set: H1 hero, H2 feature/detail, H3 lifestyle, H4 comparison, and H5 offer/trust CTA.
---

# Five Hero Images

Use this skill when the user asks for five main product images or a complete hero image set. First create one shared Campaign Style Lock covering palette, lighting, typography, background, product scale, and layout. Then create exactly five prompts:

1. H1: clear product hero / primary listing image.
2. H2: key feature, material, or craftsmanship detail.
3. H3: realistic target-user lifestyle scene.
4. H4: honest comparison or problem/solution visual.
5. H5: benefits, guarantee, shipping, or CTA composition.

Use the built-in `image_gen.imagegen` by default. In independent relay mode, use the installed relay image skill/helper. Save each result, verify every path, and return a numbered Markdown preview list.

In relay mode, call `generate_image.py` once per H1-H5 prompt with the product reference image and the required output size. Do not use `--n 5` with one prompt: the five images have different jobs and need five prompts.
