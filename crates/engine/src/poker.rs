use serde::{Deserialize, Serialize};

pub const CATEGORIES: [&str; 9] = [
    "High card",
    "One pair",
    "Two pair",
    "Three of a kind",
    "Straight",
    "Flush",
    "Full house",
    "Four of a kind",
    "Straight flush",
];
pub const PAYOUTS: [u32; 9] = [2, 8, 20, 35, 55, 75, 110, 200, 400];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hand {
    pub cards: [u8; 5],
    pub rank: u32,
    pub category: u8,
    pub reward: u32,
}
impl Hand {
    pub fn name(&self) -> &'static str {
        if self.category == 8 && (self.rank >> 16) & 15 == 14 {
            "Royal flush"
        } else {
            CATEGORIES[self.category as usize]
        }
    }
}

/// Precondition: five distinct card IDs in 0..52. Public commands validate this.
/// Rank is category followed by five kicker nibbles; larger wins.
pub fn evaluate(cards: [u8; 5]) -> Hand {
    let mut counts = [0u8; 15];
    let mut mask = 0u16;
    let suit = cards[0] / 13;
    let mut flush = true;
    for c in cards {
        let r = (c % 13 + 2) as usize;
        counts[r] += 1;
        mask |= 1 << (r - 2);
        flush &= c / 13 == suit;
    }
    let mut straight = 0;
    for high in (6..=14).rev() {
        let needed = 0b11111 << (high - 6);
        if mask & needed == needed {
            straight = high as u8;
            break;
        }
    }
    if straight == 0 && mask & 0b1000000001111 == 0b1000000001111 {
        straight = 5;
    }
    let mut groups = [(0u8, 0u8); 5];
    let mut len = 0;
    for r in (2..=14).rev() {
        if counts[r] > 0 {
            groups[len] = (counts[r], r as u8);
            len += 1;
        }
    }
    // At most five elements; no allocation or general-purpose sorting.
    for i in 1..len {
        let x = groups[i];
        let mut j = i;
        while j > 0 && groups[j - 1] < x {
            groups[j] = groups[j - 1];
            j -= 1;
        }
        groups[j] = x;
    }
    let category = if flush && straight > 0 {
        8
    } else if groups[0].0 == 4 {
        7
    } else if groups[0].0 == 3 && groups[1].0 == 2 {
        6
    } else if flush {
        5
    } else if straight > 0 {
        4
    } else if groups[0].0 == 3 {
        3
    } else if groups[0].0 == 2 && groups[1].0 == 2 {
        2
    } else if groups[0].0 == 2 {
        1
    } else {
        0
    };
    let mut kickers = [0u8; 5];
    if category == 4 || category == 8 {
        kickers[0] = straight;
    } else {
        for i in 0..len {
            kickers[i] = groups[i].1;
        }
    }
    let mut rank = category as u32;
    for k in kickers {
        rank = (rank << 4) | k as u32;
    }
    let reward = if category == 8 && straight == 14 {
        600
    } else {
        PAYOUTS[category as usize]
    };
    Hand {
        cards,
        rank,
        category,
        reward,
    }
}

pub fn best(cards: &[u8]) -> Option<Hand> {
    if cards.len() < 5 {
        return None;
    }
    let mut result: Option<Hand> = None;
    for a in 0..cards.len() - 4 {
        for b in a + 1..cards.len() - 3 {
            for c in b + 1..cards.len() - 2 {
                for d in c + 1..cards.len() - 1 {
                    for e in d + 1..cards.len() {
                        let hand = evaluate([cards[a], cards[b], cards[c], cards[d], cards[e]]);
                        if result.is_none_or(|old| hand.rank > old.rank) {
                            result = Some(hand);
                        }
                    }
                }
            }
        }
    }
    result
}

pub fn valid_cards(cards: &[u8]) -> bool {
    let mut mask = 0u64;
    for &c in cards {
        if c >= 52 || mask & (1u64 << c) != 0 {
            return false;
        }
        mask |= 1u64 << c;
    }
    true
}
