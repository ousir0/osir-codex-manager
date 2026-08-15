---
name: ecom-detail-set
description: Create a complete e-commerce detail-page image set with seven to nine images, using a shared Campaign Style Lock and conversion-oriented sequence.
---

# E-commerce Detail Set

Use this skill when the user asks for a product detail page, PDP, Amazon/Shopify detail set, or a full e-commerce image package.

- If the user specifies 7, generate exactly 7 detail images.
- If the user specifies 8, generate exactly 8 detail images.
- If the user specifies 9 or says complete/full, generate exactly 9 detail images.
- If no count is specified, default to 9.

Build one Campaign Style Lock first, then map the sequence to the requested count. The complete nine-image sequence is: D1 promise/target user, D2 pain point, D3 mechanism, D4 benefits infographic, D5 usage steps, D6 scenarios, D7 comparison, D8 trust/material/quality, D9 FAQ/risk reversal/CTA. For seven or eight images, combine adjacent low-risk sections without losing product truthfulness.

Use the built-in `image_gen.imagegen` by default. In independent relay mode, use the installed relay image skill/helper and configured model. Save and verify every output image, preferably under the task `outputs` directory, then return a numbered Markdown preview list and the final file paths.

The 25 reusable scene definitions are in `references/templates/`. Load only the templates needed for the requested product and count. In relay mode, call the bundled `generate_image.py` once per selected prompt so each output can be verified independently.
