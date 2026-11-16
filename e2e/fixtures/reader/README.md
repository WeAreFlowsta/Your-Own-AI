# Made-up documents for the document reader's tests

Every name, address, number and date here is invented. `make-scans.py OUT_DIR`
draws them (Pillow, DejaVu fonts) and writes the text each page holds to
`truth.json`:

- `1-letter.pdf`: a one-page letter as an image-only PDF (no text layer), the
  way a scanner saves it. Holds the figures 186.50 and 40.00.
- `3-form.png`: a filled-in form. Holds the postcode GL5 2HT.
- `4-photo.jpg`: a poor phone photo of a notice: tilted, blurred, shadowed,
  grainy. Holds the date 18 June 2026.
