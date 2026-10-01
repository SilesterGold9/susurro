# Onboarding revamp plan

Where setup goes next, grounded in what the best flows do and what
our architecture allows. Local-first throughout: no accounts, no
teams, no cloud sync to hide behind.

## What the best do

Wispr Flow asks intent first (dictation, notetaker, or both) and
tails the rest of the path on the answer. Permission screens prime
before they ask and auto-advance on grant. A guided test dictation
is the interactive moment, never a video. A style quiz (Formal,
Casual, Excited per category) personalizes with at most three
questions, each changing real behavior. Progress cues keep it
brief, every step skips, setup relaunches from Help, and locked
pages gate with a Get Started path instead of dead ends. Apple and
the wider literature agree: teach through doing, personalize with
three questions max, earn permissions after first value, keep it
short and skippable.

## Our revamp, in order

Built 2026-10-01, six screens:

1. Intent question. Docs, messages, or both. `apply_intent`
   writes real profiles for the category patterns, so the answer
   changes behavior. Skippable by advancing untouched.
2. Requirements, model, speed check. Whisper, model plus checksum,
   paste tools, Ollama presence plus model, bench tier beside the
   download with live percentage progress.
3. Hotkey pick. Three named choices, snippet for Hyprland,
   persisted everywhere on finish.
4. Style quiz. Formal, Casual, Verbatim cards writing profiles for
   the intent categories on tap. The Style page keeps managing
   them after.
5. Test dictation. Six seconds with mic priming copy, the
   interactive moment, transcript on screen.
6. Done plus data. Summary of everything picked, start
   dictating, and the erase-my-data control with confirm.

Progress dots across the top, skip on every screen via next and
back, replay from Help. Locked pages skipped deliberately.

## Non-goals

Sample data (our history is the content), social proof, referral
mechanics, streak challenges. Those sell a network; we sell a tool
that works offline.
