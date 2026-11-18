# 60-second demo script

Phone-sized window (390 × 844), light theme, sound on. About 150 spoken words.
Say "preview" out loud for anything from the state page. Never say a payout or a refund happened.

## Before you record

1. `bash scripts/demo.sh`. Page: `http://127.0.0.1:5173`.
2. Delete any old `~/Downloads/vaca-refund-key.txt` so the new file keeps its name.
3. Open three tabs and pin them in this order:
   - Home: `http://127.0.0.1:5173/#/`
   - The pot: `http://127.0.0.1:5173/#/pots/2589c3b7-d743-4fdf-b96a-93e0e8f4f7bf` ("Regtest payout", 100 of 1,000 sats, BOLT12)
   - Previews: `http://127.0.0.1:5173/states.html?__state=partly-funded`. Press `H` to hide the switcher.
4. Optional, if the regtest mint is up: make a 100 sat token with `node scripts/mint-test-token.mjs 100 ~/Downloads/demo-token.cashu` and pay its invoice from node A. This turns beat 3 into a real contribution. Without it, stop beat 3 at the checkbox.

## The beats

| Time | Screen | Do | Say |
|---|---|---|---|
| 0:00–0:08 | Home | Hold on the headline, then tap the "Regtest payout" row. | "Vaca is a pot where everyone chips in, or nobody pays. If the target is met, it pays a Lightning invoice. If not, everyone gets their own sats back." |
| 0:08–0:20 | The pot | Slow scroll over the card: amount, countdown, destination, mint. | "This pot wants 1,000 sats before the deadline. some sats are already locked, and you can see the time left, who it pays and which mint holds the money. The fee reserve counts toward the target." |
| 0:20–0:34 | Contribute | Tap "Create refund key and download it". Show the download. Tick the box. (Optional: pick the token and the key file, tap "Lock sats in this pot", and let the bar fill.) | "To contribute, I make a refund key. It downloads as a file and never appears on screen. Lose it, lose your refund, so I back it up first. Then I lock sats. The mint, not Vaca, enforces the refund." |
| 0:34–0:44 | How it works | Tap "How it works". Scroll the diagram to "who can spend what". | "Lock, target met, pay. Or the deadline passes and you reclaim. After the lock time both keys can spend, so reclaim fast. And today the server's key alone can spend, so this is test money." |
| 0:44–0:54 | Previews | Switch to the previews tab. Tap "Preview: add 5,000 sats" until the churn fills and "Target reached" shows. | "In a preview, when the pot covers the target plus the fee reserve, the churn fills and it's ready to pay out." |
| 0:54–1:00 | Previews | Press `→` to "Refunds open" (labelled preview). | "If the deadline passes first, each person reclaims their own sats. Nobody holds the money hostage." |

## Guardrails

- The target-reached and refund screens are previews. Say so, and don't click "Pay out now" or "Reclaim sats" on camera.
- Don't show the key file's contents or open it.
- If the server is down, the page says "Can't reach the Vaca server". Start `scripts/demo.sh` again and retake.
