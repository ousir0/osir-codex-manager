---
name: ecom-single-image
description: Generate one focused e-commerce image from a product photo or description, including hero shots, lifestyle scenes, detail macros, posters, and social creatives.
---

# E-commerce Single Image

Use this skill when the user asks for one product image or one focused visual. Preserve the product identity from any reference image and state the intended channel, aspect ratio, subject placement, lighting, and copy constraints.

Use the built-in `image_gen.imagegen` by default. When the user has enabled the independent relay image mode, use the installed `imagegen-relay` skill/helper and its configured model instead. Save the final image and return a verified absolute Markdown preview path.

For relay mode, the bundled e-commerce generator is available at `~/.codex/skills/ecom-single-image/scripts/generate_image.py`; it reads the manager configuration and supports reference images, size, quality, and output format.
