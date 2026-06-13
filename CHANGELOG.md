# Changelog

## v0.7.0 — 2026-06-13

pulse-silence-gate: gate silence-nudge on hearing_confirmed_in_window; deaf/ambiguous windows suppressed

## v0.6.0 — 2026-06-13

pulse-deaf-escalation: add deaf-device escalation reusing distress ladder; debounced, best-effort recovery note

## v0.2.0 — 2026-06-03

Per-interaction notifications would make jsy's phone buzz all day and teach
him to ignore it. The digest is the opposite: `wintermute-reach` aggregates
`wm.presence.*` events across the day and delivers a single calm summary at a
configured time — "Mom talked to wintermute 4 times today, last at 6:12pm" —
plus a line if a silence window was flagged. Reassurance, batched, opt-in.
