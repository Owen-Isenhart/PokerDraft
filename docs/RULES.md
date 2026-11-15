# Poker Draft rules — version 1

All currency is fictional, integer, nonredeemable game points. No wagering, payment processing or entry fees.

## Match and information

Default: six seats, 100 starting credits, 52 distinct cards, one 1,000 ms auction per card. Configurable seats 2–8, auction window 250–10,000 ms, first- or second-price payment. Every thirteenth award is followed immediately by an automatic showdown. The final showdown is followed by liquidation and ranking. Bots never need to send a showdown packet under a short deadline.

Card ID = suit * 13 + rank index: suits clubs, diamonds, hearts, spades; ranks 2 through ace. Deck is a Fisher–Yates shuffle using ChaCha20 seeded with 32 random bytes. Initial commitment is SHA256 of that seed. Seed is disclosed only on completion or abort; seed and rules version reproduce the complete deck. Ownership, inventories, balances, scores, resolved prices and cash-ins are public. Open bids, bidder counts, unaccepted actions and future cards are private. Bid amounts are not disclosed after resolution either; only the clearing price is public. The operator is trusted with live secrets.

## Auction

One immutable bid per seated bot per auction, bound to match ID, round and a unique request ID. Whole nonnegative credits only; 0 is a pass. A positive bid must be at least reserve 1, at most current balance, and inventory must contain fewer than ten cards. A missing bid is a pass. Validation failures do not mutate state or spend credits. Cashing in can increase liquidity but never changes an already accepted bid. Bids are never exposed through snapshots, event logs, errors or acknowledgements to another bot.

At deadline, largest positive bid wins. Ties use a reproducible domain-separated keyed hash of seed, round and tied seat; network arrival order has no effect. In second-price mode price is max(1, second largest positive bid), including tied highest bids; a lone bidder pays 1. In first-price mode price is the winning bid. Paid credits enter the showdown pot. If nobody bids, the card is burned permanently (never reinserted), so every match terminates after exactly 52 auctions. Inventory cap is checked again at settlement. There is no penalty for an invalid bid; invalid input is rejected, and abuse is rate limited.

## Cash-in

During open auctions a bot may atomically consume exactly five distinct cards from its own inventory. The five-card poker category determines both new spendable credits and earned score:

| Category | Reward |
| --- | ---: |
| High card | 2 |
| One pair | 8 |
| Two pair | 20 |
| Three of a kind | 35 |
| Straight | 55 |
| Flush | 75 |
| Full house | 110 |
| Four of a kind | 200 |
| Straight flush | 400 |
| Royal flush | 600 |

Ace can be low only in A2345. Standard lexicographic kickers break poker ties; suits have no ranking. Consumed/burned cards never return to the deck. Credits cannot go negative. Earned score is cumulative cash-in and showdown rewards; spending does not reduce score. Balance is a purchasing resource, not the ranking metric.

## Showdown and finish

Each showdown adds 40 credits to the pot. Every seat with at least five cards automatically enters its strongest five-card hand, consuming those five cards whether it wins or loses. This prevents repeatedly scoring the same cards. Best poker rank wins the entire pot. Exact poker ties split integer credits evenly; remainder credits go to tied seats in deterministic shuffled tie order. Payout increases both balance and score. If no seat qualifies, pot carries to the next showdown. After round 52, leftover inventories are automatically cashed strongest five at a time; inventories under five have zero salvage. Any unclaimed pot is retired. Final rank is earned score, with exact equal scores tied. Ratings use these tied ranks without seat-order bias.

## Lifecycle

Rolling lobby opens for 30 seconds, starts immediately when full, or fills remaining seats with clearly marked house agents on timeout. No all-house rated matches. One live match per bot and at most one active bot version per team in queues or tournaments. Disconnect does not stop a match: absent bids pass; reconnect retrieves a snapshot. A restarted server aborts in-progress matches, exposes their seeds and reason, and does not rate them; completed matches and tournament results are atomic and durable. Graceful shutdown follows the same policy. Tournament rounds advance only from completed results, and an aborted pairing is rescheduled.

Ratings are pairwise multiplayer Elo, initial 1500, K=32 normalized by number of opponents, with tied scores = 0.5 and one atomic update per completed competitive match. House agents never receive leaderboard ranks and house-only matches do not affect ratings. A new named bot version starts at 1500 while older results remain browsable.

Tournaments use seeded Swiss pairings (adjacent score groups, avoiding repeats where possible), a power-of-two top cut and single elimination. Tournament matches are heads-up; ties require a replay with a fresh seed, never arbitrary advancement. Equal Swiss points use Buchholz then seed order. Odd fields receive one bye per entrant before any repeat. Finals are best of three decisive games. Practice simulations do not alter ratings.
