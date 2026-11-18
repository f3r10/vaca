# 60-second demo script

Phone-sized window (390 × 844), light theme, sound on. About 150 spoken words.
Say "preview" out loud for anything from the state page. Never say a payout or a refund happened.

## Before you record

1. `bash scripts/demo.sh`. Page: `http://127.0.0.1:5173`.
2. Delete any old `~/Downloads/vaca-refund-key.txt` so the new file keeps its name.
3. Open three tabs and pin them in this order:
   - Home: `http://127.0.0.1:5173/#/`
   - The pot: a **fresh pot on the test mint**, created just before recording (target 1,000 sats, destination a lab-node invoice so it gets a fee quote). Start the server with `VACA_MINT_URL=https://testnut.cashu.space` so new pots use it. The old "Regtest payout" pot also works, but you must pay its invoice from node A (`./scripts/cli a bolt11-send <invoice>`).
   - Previews: `http://127.0.0.1:5173/states.html?__state=partly-funded`. Press `H` to hide the switcher.
4. Keep your saved refund key file (`~/Downloads/.../vaca-refund-key.txt`) handy. Beat 3 asks the page to read it once. No token is needed: the test mint pays its own invoice. Without it, stop beat 3 at the checkbox.

## The beats

| Time | Screen | Do | Say |
|---|---|---|---|
| 0:00–0:08 | Home | Hold on the headline, then tap the "Regtest payout" row. | "Vaca is a pot where everyone chips in, or nobody pays. If the target is met, it pays a Lightning invoice. If not, everyone gets their own sats back." |
| 0:08–0:20 | The pot | Slow scroll over the card: amount, countdown, destination, mint. | "This pot wants 1,000 sats before the deadline. some sats are already locked, and you can see the time left, who it pays and which mint holds the money. The fee reserve counts toward the target." |
| 0:20–0:34 | Contribute | Tap "Create refund key and download it". Choose the saved file ("Backup confirmed"). Keep "Pay with Lightning", leave 100, tap "Pay 102 sats with Lightning", and let the bar fill. | "To contribute I make a refund key and choose the saved file once, so Vaca checks that my refund works. Then I pay one Lightning invoice. This is a test mint, so it pays itself. The sats lock to the pot and the bar fills." |
| 0:34–0:44 | How it works | Tap "How it works". Scroll the diagram to "who can spend what". | "Lock, target met, pay. Or the deadline passes and you reclaim. After the lock time both keys can spend, so reclaim fast. And today the server's key alone can spend, so this is test money." |
| 0:44–0:54 | Previews | Switch to the previews tab. Tap "Preview: add 5,000 sats" until the churn fills and "Target reached" shows. | "In a preview, when the pot covers the target plus the fee reserve, the churn fills and it's ready to pay out." |
| 0:54–1:00 | Previews | Press `→` to "Refunds open" (labelled preview). | "If the deadline passes first, each person reclaims their own sats. Nobody holds the money hostage." |

## Guardrails

- The target-reached and refund screens are previews. Say so, and don't click "Pay out now" or "Reclaim sats" on camera.
- Don't show the key file's contents or open it.
- If the server is down, the page says "Can't reach the Vaca server". Start `scripts/demo.sh` again and retake.
