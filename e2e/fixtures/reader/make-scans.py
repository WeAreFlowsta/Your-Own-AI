#!/usr/bin/env python3
"""Four made-up documents as image-only PDFs (no text layer) and PNGs, with
the text each one holds: a letter, an invoice with a table, a form, and a
poor phone photo of a second letter. Usage: make-scans.py OUT_DIR"""
import sys, json, random
from PIL import Image, ImageDraw, ImageFont, ImageFilter, ImageEnhance
out = sys.argv[1]; random.seed(7)
W, H = 1240, 1754  # A4 at 150 dpi
F = "/usr/share/fonts/truetype/dejavu/"
font = lambda n, s: ImageFont.truetype(F + n, s)
R, B, S = font("DejaVuSerif.ttf", 26), font("DejaVuSans-Bold.ttf", 30), font("DejaVuSans.ttf", 24)
truth = {}
def page(): im = Image.new("RGB", (W, H), "white"); return im, ImageDraw.Draw(im)
def lines(d, x, y, ls, f, gap=12):
    for l in ls:
        d.text((x, y), l, fill="black", font=f); y += f.size + gap
    return y

# 1. a letter
letter = ["Harbour Lane Dental Practice", "14 Harbour Lane, Whitby YO21 3PU", "", "3 March 2026", "", "Dear Ms Okafor,", "",
 "Thank you for attending your appointment on 27 February. This letter",
 "confirms the treatment plan we discussed. Two fillings are needed on the",
 "upper left side, and we recommend a hygienist visit every six months.", "",
 "Your next appointment is on Tuesday 14 April 2026 at 9:40 am with",
 "Dr Lindqvist. Please arrive ten minutes early to update your medical",
 "history form. If you need to change the time, call us on 01947 555 0182",
 "at least 48 hours before the appointment.", "",
 "The estimated cost of the treatment is 186.50 pounds. A deposit of",
 "40.00 pounds is payable when you confirm the booking.", "", "Yours sincerely,", "", "Priya Raman", "Practice Manager"]
im, d = page(); lines(d, 130, 150, letter, R); im.save(f"{out}/1-letter.png"); truth["1-letter"] = " ".join(letter)

# 2. an invoice with a table
im, d = page()
head = ["Northfield Garden Supplies", "INVOICE No. 20417", "Date: 12 May 2026", "Bill to: Rowan Achterberg, 8 Mill Road, Ely"]
y = lines(d, 110, 120, head[:1], B); y = lines(d, 110, y + 10, head[1:], S)
cols = [110, 620, 780, 950]; rows = [["Item", "Qty", "Unit price", "Total"], ["Compost 50 litre bag", "6", "7.25", "43.50"], ["Oak raised bed kit", "2", "64.00", "128.00"],
 ["Tomato seedlings tray", "3", "5.80", "17.40"], ["Watering can 9 litre", "1", "14.95", "14.95"], ["Garden twine 100 m", "4", "2.10", "8.40"]]
y += 40; top = y
for r in rows:
    for c, t in zip(cols, r): d.text((c + 10, y + 12), t, fill="black", font=B if r is rows[0] else S)
    y += 56; d.line((110, y, 1130, y), fill="black", width=2)
d.rectangle((110, top, 1130, y), outline="black", width=2)
for c in cols[1:]: d.line((c, top, c, y), fill="black", width=2)
tail = ["Subtotal: 212.25", "Delivery: 9.50", "VAT at 20%: 44.35", "Amount due: 266.10", "Payment due within 14 days. Account 40718832, sort code 20-45-77."]
lines(d, 110, y + 40, tail, S); im.save(f"{out}/2-invoice.png")
truth["2-invoice"] = " ".join(head + [" ".join(r) for r in rows] + tail)
truth["2-invoice-rows"] = [r for r in rows[1:]]

# 3. a form
im, d = page(); y = lines(d, 110, 110, ["Library Membership Application"], B) + 30
fields = [("Full name", "Tomasz Wieczorek"), ("Date of birth", "09/11/1987"), ("Address", "Flat 3, 22 Canal Street"), ("Town", "Stroud"), ("Postcode", "GL5 2HT"),
 ("Telephone", "07700 900 461"), ("Email", "t.wieczorek@example.org"), ("Membership type", "Adult, 12 months"), ("Card number", "LB-0048-7731")]
for k, v in fields:
    d.text((110, y), k + ":", fill="black", font=S); d.rectangle((420, y - 8, 1120, y + 40), outline="black", width=2); d.text((432, y), v, fill="black", font=S); y += 74
checks = [("Email reminders", True), ("Large print books", False), ("Home delivery", True)]
for k, on in checks:
    d.rectangle((110, y, 140, y + 30), outline="black", width=2)
    if on: d.line((114, y + 15, 124, y + 26), fill="black", width=4); d.line((124, y + 26, 138, y + 4), fill="black", width=4)
    d.text((160, y), k, fill="black", font=S); y += 56
foot = ["I agree to the library rules. Signed on 2 June 2026."]; lines(d, 110, y + 20, foot, S); im.save(f"{out}/3-form.png")
truth["3-form"] = " ".join(["Library Membership Application"] + [f"{k}: {v}" for k, v in fields] + [k for k, _ in checks] + foot)

# 4. a poor phone photo of another letter
note = ["Kestrel Row Residents Association", "", "Notice of annual meeting", "",
 "The annual meeting will be held on Thursday 18 June 2026 at 7:30 pm",
 "in the community room at 5 Kestrel Row. All residents are welcome.", "",
 "Agenda", "1. Report from the chair, Imogen Castellanos",
 "2. Accounts for the year: income 4,310 and spending 3,865",
 "3. Bin store repairs and the quote of 1,240 from Dalby Builders",
 "4. Election of the committee", "5. Any other business", "",
 "Please send questions to the secretary before 11 June.", "Refreshments will be served from 7:00 pm."]
im, d = page(); lines(d, 140, 170, note, R)
im = im.rotate(3.2, resample=Image.BICUBIC, expand=True, fillcolor=(120, 110, 100))
w, h = im.size
im = im.transform((w, h), Image.QUAD, (0, 0, -40, h, w + 60, h - 30, w - 20, 40), resample=Image.BICUBIC, fillcolor=(120, 110, 100))
shade = Image.linear_gradient("L").resize((w, h)).rotate(35, expand=False)
im = Image.composite(im, ImageEnhance.Brightness(im).enhance(0.62), shade)
im = im.filter(ImageFilter.GaussianBlur(1.3))
px = im.load()
for _ in range(60000):
    x, y0 = random.randrange(w), random.randrange(h); n = random.randint(-45, 45); r, g, b = px[x, y0]; px[x, y0] = (max(0, min(255, r + n)), max(0, min(255, g + n)), max(0, min(255, b + n)))
im = ImageEnhance.Contrast(im).enhance(0.8); im.save(f"{out}/4-photo.jpg", quality=38); im = Image.open(f"{out}/4-photo.jpg"); im.save(f"{out}/4-photo.png")
truth["4-photo"] = " ".join(note)

# 5. a statement over two pages: the figure asked about is on the second
first = ["Fenwick Street Allotment Society", "Plot holder statement for 2026", "", "Plot 31, Mr Dariusz Halloran", "",
 "This statement covers the year from 1 January to 31 December 2026.",
 "Rent and water charges are listed on this page. Deposits, refunds and",
 "the balance carried forward are on the second page.", "",
 "Annual rent for a full plot: 58.00", "Water charge: 14.50", "Shed licence: 9.00", "",
 "Charges for the year: 81.50", "", "Continued on page 2."]
second = ["Fenwick Street Allotment Society", "Plot 31 statement, page 2", "",
 "Key deposit held: 20.00", "Refund for the 2025 overpayment: 6.25", "Paid on 4 February 2026: 50.00", "",
 "Balance carried forward: 312.75", "", "Please pay by 30 April 2026. Account 51904463, sort code 30-11-62.",
 "Queries to the treasurer, Odalys Whitcombe."]
im1, d = page(); lines(d, 130, 150, first, R); im2, d = page(); lines(d, 130, 150, second, R)
im1.save(f"{out}/5-two-pages.pdf", save_all=True, append_images=[im2], resolution=150)
truth["5-two-pages"] = " ".join(first + second)

for n in ["1-letter", "2-invoice", "3-form", "4-photo"]:
    Image.open(f"{out}/{n}.png").convert("RGB").save(f"{out}/{n}.pdf", resolution=150)
json.dump(truth, open(f"{out}/truth.json", "w"), indent=1)
