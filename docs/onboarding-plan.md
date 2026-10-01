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

1. Intent question (new screen 0). What brings you here: docs,
   messages, or both. Docs seeds a formal profile for document
   apps, messages seeds casual for chat apps, both seeds both.
   One question, real behavior change, skippable.
2. Progress indicator. Step dots across the top of every screen,
   four today, five with intent. Setup must look brief to feel brief.
3. Style quiz folded in. The existing Formal, Casual, Verbatim
   cards move into onboarding as the tone question, writing a real
   profile for the intent categories. The Style page keeps managing
   them after.
4. Mic priming before the test. One line explaining the test needs
   the mic, then the existing six-second test as the interactive
   moment. No separate permission API exists to call; the priming
   is the honest version.
5. Skip on every screen plus replay from Help. Done already for
   skip (next buttons); replay lands with this plan.
6. Data deletion on the Done screen. Implemented now, see below.
7. Locked pages: skipped deliberately. No teams, no paywalls, no
   gates. Every page works from first launch.

## Non-goals

Sample data (our history is the content), social proof, referral
mechanics, streak challenges. Those sell a network; we sell a tool
that works offline.
