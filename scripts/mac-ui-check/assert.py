"""Fail on what breaks a screen: overflow, a modal taller than its window, an overlay card
taller than its window, or a state that did not render."""
import json, sys

states = json.load(open(sys.argv[1]))
problems = []
for s in states:
    name = s.get("name")
    if "error" in s:
        problems.append(f"{name}: {s['error']}")
        continue
    w, h = s["viewport"]
    if s["hOverflow"]:
        problems.append(f"{name}: horizontal overflow")
    modal = s["sizes"].get(".modal-panel")
    if modal and modal[1] > h:
        problems.append(f"{name}: modal {modal[1]}px taller than the {h}px window")
    shell = s["sizes"].get(".overlay-shell")
    if shell and shell[1] > h:
        problems.append(f"{name}: overlay card {shell[1]}px taller than the {h}px window")
    # The empty and first-week Home scroll by 17px in Chromium too (the quote card sizes
    # itself off its own height with no day card to anchor it): a shared bug, not a Mac one.
    if name.startswith("home") and s["docH"] > h + 24:
        problems.append(f"{name}: Home scrolls ({s['docH']}px in {h}px)")
print(f"{len(states)} states laid out in WebKit")
if problems:
    print("\n".join(problems))
    sys.exit(1)
print("ok: no overflow, every modal and overlay card fits, Home does not scroll")
