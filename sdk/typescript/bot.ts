/** Node 22+; no npm install. node --experimental-strip-types sdk/typescript/bot.ts
 * Use only a BOT credential. Edit decideBid and chooseCash to change strategy.
 */
import { randomUUID } from "node:crypto";
const url = process.env.POKERDRAFT_URL ?? "http://localhost:3000";
const parsed = new URL(url);
if (
  !["http:", "https:", "ws:", "wss:"].includes(parsed.protocol) ||
  parsed.username ||
  parsed.password ||
  parsed.search ||
  parsed.hash
)
  throw new Error("Use a credential-free arena URL without query or fragment.");
const local = ["localhost", "127.0.0.1", "[::1]"].includes(parsed.hostname);
if (
  ["http:", "ws:"].includes(parsed.protocol) &&
  !local &&
  process.env.POKERDRAFT_ALLOW_INSECURE !== "true"
)
  throw new Error(
    "Use HTTPS/WSS outside loopback. Insecure development requires POKERDRAFT_ALLOW_INSECURE=true.",
  );
const token = process.env.POKERDRAFT_TOKEN;
if (!token) throw new Error("Set POKERDRAFT_TOKEN to your bot credential.");
type Player = { id: string; cards: number[]; balance: number; score: number };
type Snapshot = {
  round: number;
  card: number | null;
  finished: boolean;
  players: Player[];
};
function rank(cards: number[]): { rank: number[]; reward: number } {
  const ranks = cards.map((c) => (c % 13) + 2).sort((a, b) => b - a);
  const counts = new Map<number, number>();
  for (const r of ranks) counts.set(r, (counts.get(r) ?? 0) + 1);
  const groups = [...counts]
    .map(([r, n]) => [n, r])
    .sort((a, b) => b[0] - a[0] || b[1] - a[1]);
  const flush = cards.every(
    (c) => Math.floor(c / 13) === Math.floor(cards[0] / 13),
  );
  const unique = [...counts.keys()];
  const straight =
    unique.length === 5
      ? ranks[0] - ranks[4] === 4
        ? ranks[0]
        : ranks.join(",") === "14,5,4,3,2"
          ? 5
          : 0
      : 0;
  let cat =
    flush && straight
      ? 8
      : groups[0][0] === 4
        ? 7
        : groups[0][0] === 3 && groups[1][0] === 2
          ? 6
          : flush
            ? 5
            : straight
              ? 4
              : groups[0][0] === 3
                ? 3
                : groups[0][0] === 2 && groups[1][0] === 2
                  ? 2
                  : groups[0][0] === 2
                    ? 1
                    : 0;
  return {
    rank: [
      cat,
      ...(cat === 4 || cat === 8 ? [straight] : groups.map((g) => g[1])),
    ],
    reward:
      cat === 8 && straight === 14
        ? 600
        : [2, 8, 20, 35, 55, 75, 110, 200, 400][cat],
  };
}
function better(a: number[], b: number[]): boolean {
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) > (b[i] ?? 0);
  }
  return false;
}
function best(
  cards: number[],
): { cards: number[]; reward: number; rank: number[] } | null {
  let result: { cards: number[]; reward: number; rank: number[] } | null = null;
  for (let a = 0; a < cards.length - 4; a++)
    for (let b = a + 1; b < cards.length - 3; b++)
      for (let c = b + 1; c < cards.length - 2; c++)
        for (let d = c + 1; d < cards.length - 1; d++)
          for (let e = d + 1; e < cards.length; e++) {
            const subset = [cards[a], cards[b], cards[c], cards[d], cards[e]];
            const h = rank(subset);
            if (!result || better(h.rank, result.rank))
              result = { cards: subset, ...h };
          }
  return result;
}
export function decideBid(s: Snapshot, seat: number): number {
  const p = s.players[seat],
    card = s.card;
  if (card === null || p.cards.length >= 10) return 0;
  let value = 2 + ((s.round + seat * 7) % 4);
  for (const c of p.cards) {
    if (c % 13 === card % 13) value += 5;
    if (Math.floor(c / 13) === Math.floor(card / 13)) value++;
    if (Math.abs((c % 13) - (card % 13)) <= 2) value++;
  }
  const h = best([...p.cards, card]);
  if (h) value += Math.floor(h.reward / 5);
  return Math.min(value, Math.floor(p.balance / 3) + 1, p.balance);
}
export function chooseCash(p: Player): number[] | null {
  const h = best(p.cards);
  return h && (h.reward >= 35 || p.cards.length >= 9) ? h.cards : null;
}
let retries = 0;
let stopping = false;
const acted = new Set<string>(),
  completed = new Set<string>();
function connect() {
  const ws = new WebSocket(
    url.replace(/^http/, "ws").replace(/\/$/, "") + "/ws",
  );
  let bot = "";
  let lastJoin = 0;
  const pending = new Map<
    string,
    {
      resolve: (v: any) => void;
      reject: (e: Error) => void;
      timer: ReturnType<typeof setTimeout>;
    }
  >();
  let chain = Promise.resolve();
  function send(
    op: string,
    fields: Record<string, unknown> = {},
  ): Promise<any> {
    return new Promise((resolve, reject) => {
      const id = randomUUID();
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error("ack_timeout"));
      }, 10000);
      pending.set(id, { resolve, reject, timer });
      ws.send(JSON.stringify({ id, op, ...fields }));
    });
  }
  function join() {
    return send("join").catch((e) => {
      if (!["already_active", "queue_paused"].includes(e.message)) throw e;
    });
  }
  async function play(m: any) {
    const s: Snapshot = m.snapshot;
    const seat = s.players.findIndex((p) => p.id === bot);
    if (seat < 0) return;
    if (s.finished) {
      if (completed.has(m.id)) return;
      completed.add(m.id);
      if (completed.size > 128)
        completed.delete(completed.values().next().value!);
      console.log(
        `Match ${m.id.slice(0, 8)} ${m.status}: score ${s.players[seat].score}`,
      );
      if (process.argv.includes("--once")) {
        stopping = true;
        ws.close();
        return;
      }
      await join();
      return;
    }
    const key = `${m.id}:${s.round}`;
    if (acted.has(key)) return;
    acted.add(key);
    if (acted.size > 1024) {
      acted.clear();
      acted.add(key);
    }
    const cards = chooseCash(s.players[seat]);
    if (cards) {
      await send("cash", { match_id: m.id, round: s.round, cards });
      const reward = rank(cards).reward;
      s.players[seat].cards = s.players[seat].cards.filter(
        (c) => !cards.includes(c),
      );
      s.players[seat].balance += reward;
    }
    await send("bid", {
      match_id: m.id,
      round: s.round,
      amount: decideBid(s, seat),
    });
  }
  const heartbeat = setInterval(() => {
    if (ws.readyState === WebSocket.OPEN) send("ping").catch(() => ws.close());
  }, 20000);
  ws.onopen = () => {
    chain = send("auth", { token })
      .then(async (a) => {
        bot = a.bot_id;
        retries = 0;
        console.log("Connected to arena.");
        if (a.active_match) {
          const r = await send("watch", { match_id: a.active_match });
          await play(r.match);
        } else await join();
      })
      .catch(failure);
  };
  function failure(e: Error) {
    if (e.message === "bot_auth_required") {
      stopping = true;
      console.error(
        "Bot credential rejected. Create or rotate it in the workshop.",
      );
      ws.close();
    } else if (
      ![
        "already_bid",
        "stale_round",
        "deadline_passed",
        "match_not_live",
        "invalid_cards",
      ].includes(e.message)
    ) {
      console.error(`Connection error: ${e.message}`);
      ws.close();
    }
  }
  ws.onmessage = (e) => {
    let m: any;
    try {
      const text = String(e.data);
      if (text.length > 2_000_000) throw new Error();
      m = JSON.parse(text);
      if (!m || typeof m !== "object" || Array.isArray(m)) throw new Error();
    } catch {
      ws.close();
      return;
    }
    if (m.type === "ack") {
      const p = pending.get(m.id);
      if (p) {
        clearTimeout(p.timer);
        pending.delete(m.id);
        m.ok ? p.resolve(m.result) : p.reject(new Error(m.error));
      }
    } else if (
      m.type === "directory" &&
      bot &&
      !m.paused &&
      Date.now() - lastJoin > 3000
    ) {
      lastJoin = Date.now();
      const queued = m.lobby.bots.some((p: any) => p.id === bot),
        active = m.matches.some((x: any) =>
          x.players.some((p: any) => p.id === bot),
        );
      if (!queued && !active) chain = chain.then(() => join()).catch(failure);
    } else if (m.type === "match") {
      chain = chain.then(() => play(m.match)).catch(failure);
    }
  };
  ws.onerror = () => ws.close();
  ws.onclose = () => {
    clearInterval(heartbeat);
    for (const p of pending.values()) {
      clearTimeout(p.timer);
      p.reject(new Error("disconnected"));
    }
    pending.clear();
    acted.clear();
    if (!stopping) setTimeout(connect, Math.min(15000, 1000 * 2 ** retries++));
  };
}
connect();
