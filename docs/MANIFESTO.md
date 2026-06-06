# The Candylane Manifesto

*Why this exists, what we believe, and the lines we will not cross.*

---

## The magic word

There is a moment, in an old story, where a child speaks one word and inherits all the power and wisdom of those who came before. No ritual. No setup. No three-hour ceremony. Just the word, and then the lightning.

That is what computing should feel like. You have built the perfect machine before — you know your tools, your keys, your hardened config, your network. You should not have to rebuild it from scratch every time you touch a new piece of hardware. You should be able to say one word and have it all again.

Candylane is that word.

---

## What we believe

**Your machine is yours.** Not Microsoft's, not a vendor's, not a telemetry pipeline's. The defaults serve someone else. You should be able to strip them away and replace them with your own, completely, on every machine you own, in one command.

**Setup is a tax, and we refuse to pay it twice.** Every hour spent debloating, configuring, and re-installing is an hour stolen from the work that matters. We pay that cost once, encode it, and never pay it again.

**Openness makes the ecosystem safer, not weaker.** We are part of the public cyber tradition. The more people who understand how systems are attacked, the more people who can defend them — and the fewer successful attacks there are. A house known to be defended is a house less likely to be robbed. We build tools that arm defenders by refusing to hide how things work.

**Trust is earned with cryptography, not granted by a server.** Candylane has no master account, no central authority that must stay online for your machines to work. Your identity is a key you hold. Your profiles are signed by you. Your secrets are encrypted to you. We can disappear tomorrow and your setup still works.

**Reproducibility without dogma.** We admire the purists, but we are not them. You can declare your machine cleanly and pin it precisely — and you can also drop in a quick script when real life demands it. Power users do imperative things. The tool should accept that instead of lecturing about it.

**Personality is not decoration — it's a moat.** Tools that are a joy to use get used. We will make Candylane delightful: a themed shell on first run, color where color helps, error messages that respect your intelligence. Lovable is a feature.

---

## Who we serve

The people who live in computers. Developers who set up a dozen environments a year. Security professionals who carry their toolkit between engagements. Homelabbers with more machines than sense. Researchers heading somewhere with no signal. Anyone who has ever opened a fresh laptop and felt their stomach sink at the hours ahead.

We serve the power user first. We will not dumb the tool down to chase a wider audience. But we will lower the floor — with plain-language explanations of what a profile does, and AI assistance for those crossing the chasm into power-user territory — without ever lowering the ceiling for the experts who are our reason for existing.

---

## The lines we will not cross

We say "no guardrails," and we mean it about your machine. Strip it, harden it, arm it, tunnel it, mod it — we do not ask and we do not judge. But "no guardrails on your own machine" is not the same as "no principles." There are exactly three things we will not host on the public lane registry:

1. **We will not host attacks on our own users.** Supply-chain poisoning, credential theft, anything designed to turn Candylane against the people who trust it. The whole point of this tool is that it gets to touch every machine you own. We will protect that trust without exception.

2. **We will not host silent exfiltration.** If a lane sends user data somewhere, it must say so plainly in its manifest. Surprises are betrayals.

3. **We will not host what is illegal in essentially every jurisdiction.** CSAM and its kin have no home here.

That is the complete list. It is the same line GitHub holds, and it is deliberately narrow. Everything else — every offensive tool, every aggressive debloat, every paranoid config, every use case that makes a corporate security team nervous — is welcome.

---

## Our promises to you

- **No telemetry.** We do not watch what you do. We could not betray your trust this way even if we wanted to, because we do not collect the data in the first place.
- **No required account.** Sync to your own Git, your own storage, your own USB stick. Candylane works with nothing of ours running anywhere.
- **No lock-in.** Profiles are plain TOML. Bundles are open formats. Your data is yours and portable. Leave whenever you want; take everything with you.
- **Reversibility by default.** Every change is logged. Most are reversible. The ones that aren't will warn you loudly before they happen. We will never quietly do something to your machine that you cannot undo or at least understand.
- **Visibility over magic.** `diff` before `pull`. You see every command, every registry key, every script, before anything runs with admin rights. The lightning strikes only after you've seen exactly what it will do.

---

## How we build

Time is what it is. **Completion is what matters.**

We ship each phase when it is solid, not when a calendar demands it. We would rather have a small thing that works perfectly than a large thing that works sometimes. The boring foundation — the state engine, the transaction log, the threat model — comes before the exciting features, because the exciting features only feel like magic when the foundation underneath them is unshakable.

We build in the open. We dogfood relentlessly. We are our own first and most demanding user. If Candylane does not make our own lives dramatically easier across our own machines, it is not ready for yours.

---

## The invitation

If you have ever wished your machine could just *be the way you like it*, everywhere, instantly — this is for you.

If you believe that open tools make us all safer, that your computer should answer to you and no one else, and that setting up a laptop should take a minute and not an afternoon — this is for you.

Say the word.

🍭

---

*This document anchors every decision Candylane makes. When a feature, a policy, or a tradeoff is in question, it is measured against what is written here. The values come first. The code reflects them.*
