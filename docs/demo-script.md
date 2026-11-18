# 60-second demo script

Recorded on the live site, https://vaca-pots.vercel.app. Phone-sized window (390 × 844),
light theme, sound on. About 150 spoken words.

The site uses a **test mint** whose invoices pay themselves, so no real sats move.
Say "test mint" or "test sats" whenever money is on screen. Never say real money moved.

## Before you record

1. Open `https://vaca-pots.vercel.app`. The first request can be slow if the API has been
   idle, so load the home page once and check that the pot list appears.
2. Create the demo pot **right before recording**, so the bar starts empty:
   - Target **10** sats, deadline 1 day away, any title (for example "Team dinner").
   - Destination: a fresh lab-node invoice. Run
     `cd ~/projects/ldk-lab && ./scripts/cli a bolt11-receive 10sat -d "demo"` and paste the
     `invoice` value. Without a destination the mint can quote, the pot cannot pay out.
   - Keep that tab open on the new pot.
3. Have your refund key file ready. Beat 3 asks the page to read it once. If you don't have
   one, start beat 3 with "Create refund key and download it" and choose the file it saves.
4. Open a second tab on `https://vaca-pots.vercel.app/#/how` for beat 5.
5. Do one dry run first, on its own pot. A 10 sat pot is full after one contribution and can
   only be paid out once, so the dry run cannot reuse the demo pot.

## The beats

| Time | Screen | Do | Say |
|---|---|---|---|
| 0:00–0:07 | Home | Hold on the headline, then tap the demo pot. | "Vaca is a pot where everyone chips in, or nobody pays. If the target is met, it pays a Lightning invoice. If not, everyone gets their own sats back." |
| 0:07–0:17 | The pot | Slow scroll over the card: target, countdown, destination, mint. | "This pot needs 10 sats before the deadline. You can see the time left, who it pays, and which mint holds the money. This is a test mint." |
| 0:17–0:32 | Contribute | Choose the key file ("Backup confirmed"). Tap "Fill the pot", then "Pay with Lightning". Let the bar fill and "Target reached" appear. | "To contribute I back up a refund key and choose the file once, so Vaca checks my refund works. Then I pay one Lightning invoice. The test mint pays it itself. The sats lock to the pot, and the target is reached." |
| 0:32–0:42 | Payout | Tap "Pay out now", then "Confirm and pay out". Wait for "Paid". | "Anyone can start the payout. The server asks the mint to pay the invoice that was fixed when the pot was created. It only says paid after the mint confirms. On this test mint that is simulated." |
| 0:42–0:52 | How it works | Switch to the second tab. Scroll to "Who can spend what, and when". | "If the deadline passes first, each person reclaims their own sats with their refund key, and the mint enforces it. After the lock time both keys can spend, so reclaim fast. Today the server's key alone can spend, so this is test money only." |
| 0:52–1:00 | Pot | Back to the paid pot. | "No smart contract, and nobody holds the money hostage. That's Vaca." |

## Optional: show a real refund

A refund needs the deadline plus 10 minutes to pass, so it cannot be done live in 60 seconds.
To show it, create a pot with a deadline 4 minutes away, fund it, **do not pay it out**, and
wait until the deadline plus 10 minutes. Then record "Refunds open", choose the key file, and
tap "Reclaim sats". The page says how many sats came back (a sat or two less than were
locked, because of the mint's input fee).

## Guardrails

- A "Paid" pot on the test mint is simulated. Say so.
- Don't show the key file's contents. The page never shows the private key, so don't open it.
- The Chrome download bar can hide the page. Dismiss it before recording.
- If the page says "Can't reach the Vaca server", the Fly API is waking up. Wait ten seconds
  and reload.
- If the test mint is down, the invoice never completes. Stop, and record the how-it-works
  and the state screens instead.
- The dev-only preview pages (`states.html`, `direction.html`) are not on the live site.
  To show a state that cannot happen live (for example "mint unreachable"), run the app locally
  with `bash scripts/demo.sh` and open `http://127.0.0.1:5173/states.html`. Say "preview".
