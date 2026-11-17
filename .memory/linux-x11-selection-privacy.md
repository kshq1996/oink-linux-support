---
name: linux-x11-selection-privacy
description: How the oink-clipboard Linux port reads the KDE privacy hint over X11 and the x11rb API traps that cost the most time
type: project
---

Facts learned while implementing the Linux clipboard privacy hint (2026-11-17):

- The Linux privacy level is the KDE convention: the owner answers a
  `ConvertSelection` request whose *target* is the atom
  `x-kde-passwordManagerHint` and writes the answer into the requestor's
  *property* atom. `crates/oink-clipboard/src/linux/privacy.rs` reads it with
  x11rb 0.13: create an `INPUT_ONLY` window, intern the property atom,
  `convert_selection(...)`, then poll for `SelectionNotify` and read the
  property.
- x11rb 0.13 argument order is `convert_selection(requestor, selection, target,
  property, time)` — the **requestor comes first**. Passing
  `(selection, target, property, requestor, time)` fails with `BadWindow`
  whose `bad_value` is the *selection* atom, which reads like a broken
  selection owner rather than a swapped argument list.
- The `property` argument is an **atom**, not a resource id: `generate_id()`
  produces a number the server rejects with `BadAtom`. Intern a real atom
  (`intern_atom`) for it; the server has no such atom otherwise.
- The selection owner must be asked for the hint *target*, and the hint
  property is written by the owner only when it accepts the conversion. A
  selection without an owner (no arboard clipboard yet) simply has no hint.
- `arboard`'s `SetExtLinux::exclude_from_history()` sets the same hint on the
  writing side, so a locally published selection is concealed too.

**Why:** these three argument/type traps produce misleading X11 errors
(`BadWindow`/`BadAtom`) and cost more time than the rest of the Linux port.
**How to apply:** keep the X11 conversation in `crates/oink-clipboard/src/linux/`
and verify it against a real X server, not a mock: `Xvfb :99` in the project
image plus `cargo test -p oink-clipboard --lib linux` exercises the round trip
(the hint is read back and asserted). See also [[linux-clipboard-grant-identity]].
