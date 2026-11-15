import React, { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { api, command } from "./api";
import type {
  Bot,
  Directory,
  Match,
  Snapshot,
  Summary,
  Tournament,
  Standings,
  GameEvent,
} from "./types";
import "./style.css";

const SUITS = ["♣", "♦", "♥", "♠"];
const RANKS = [
  "2",
  "3",
  "4",
  "5",
  "6",
  "7",
  "8",
  "9",
  "10",
  "J",
  "Q",
  "K",
  "A",
];
const REWARDS = [
  ["High card", 2],
  ["One pair", 8],
  ["Two pair", 20],
  ["Three of a kind", 35],
  ["Straight", 55],
  ["Flush", 75],
  ["Full house", 110],
  ["Four of a kind", 200],
  ["Straight flush", 400],
  ["Royal flush", 600],
];
function Card({
  id,
  small = false,
  ghost = false,
}: {
  id?: number | null;
  small?: boolean;
  ghost?: boolean;
}) {
  const suit = id == null ? 0 : Math.floor(id / 13);
  return (
    <div
      aria-label={
        id == null
          ? "Empty card slot"
          : `${RANKS[id % 13]} of ${["clubs", "diamonds", "hearts", "spades"][suit]}`
      }
      className={`card ${small ? "small" : ""} ${suit === 1 || suit === 2 ? "red" : ""} ${id == null ? "back" : ""} ${ghost ? "ghost" : ""}`}
    >
      {id == null ? (
        <span className="back-diamond">◇</span>
      ) : (
        <>
          <span className="corner">
            {RANKS[id % 13]}
            <span>{SUITS[suit]}</span>
          </span>
          <span className="suit">{SUITS[suit]}</span>
          {!small && (
            <span className="corner bottom">
              {RANKS[id % 13]}
              <span>{SUITS[suit]}</span>
            </span>
          )}
        </>
      )}
    </div>
  );
}
function Arrow() {
  return (
    <svg
      className="arrow-icon"
      aria-hidden="true"
      width="15"
      height="15"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
    >
      <path d="M5 19 19 5M5 5h14v14" />
    </svg>
  );
}
function Badge({
  children,
  green = false,
}: {
  children: React.ReactNode;
  green?: boolean;
}) {
  return <span className={`badge ${green ? "green" : ""}`}>{children}</span>;
}
function ErrorNote({ message }: { message: string }) {
  return message ? (
    <p className="error" role="alert">
      {message.replaceAll("_", " ")}
    </p>
  ) : null;
}
function useClock(period: number) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), period);
    return () => clearInterval(timer);
  }, [period]);
  return now;
}
function AuctionClock({
  match,
  snap,
  offset,
}: {
  match: Match | null;
  snap: Snapshot | null;
  offset: number;
}) {
  const now = useClock(100);
  const remaining = match?.deadline_ms
    ? Math.max(0, match.deadline_ms - now - offset)
    : 0;
  const fraction =
    snap && !snap.finished ? remaining / snap.rules.auction_ms : 0;
  return (
    <div className="auction-clock">
      <span className="eyebrow light">
        {snap?.finished ? "MATCH COMPLETE" : "BIDDING WINDOW"}
      </span>
      <div className="clock-number">
        {snap?.finished ? "52" : (remaining / 1000).toFixed(1)}
        <small>{snap?.finished ? "/52" : "s"}</small>
      </div>
      <div className="clock-track">
        <span style={{ width: `${Math.min(1, fraction) * 100}%` }} />
      </div>
      <p>
        {snap?.finished
          ? match?.status === "aborted"
            ? "Match aborted · unrated"
            : "All auctions resolved"
          : `Next showdown at card ${Math.min(52, (Math.floor((snap?.round ?? 0) / 13) + 1) * 13)}`}
      </p>
    </div>
  );
}
function LobbyCountdown({
  deadline,
  offset,
  paused,
}: {
  deadline?: number;
  offset: number;
  paused?: boolean;
}) {
  const now = useClock(1000);
  return (
    <p>
      {paused
        ? "New matches are paused."
        : `Lobby starts in ${Math.max(0, Math.ceil(((deadline ?? now) - (now + offset)) / 1000))}s. House agents fill open seats.`}
    </p>
  );
}
function PageControls({
  label,
  offset,
  total,
  size,
  onChange,
}: {
  label: string;
  offset: number;
  total: number;
  size: number;
  onChange: (n: number) => void;
}) {
  return (
    <div className="page-controls" aria-label={`${label} pagination`}>
      <span>
        {total && offset < total
          ? `${offset + 1}–${Math.min(total, offset + size)} of ${total}`
          : total
            ? "No entries on this page"
            : "0 entries"}
      </span>
      <button
        className="text-link"
        aria-label={`Previous ${label} page`}
        disabled={offset === 0}
        onClick={() => onChange(Math.max(0, offset - size))}
      >
        ← Previous
      </button>
      <button
        className="text-link"
        aria-label={`Next ${label} page`}
        disabled={offset + size >= total}
        onClick={() => onChange(offset + size)}
      >
        Next →
      </button>
    </div>
  );
}
function PreviousPairings({
  history,
  name,
}: {
  history: Tournament["history"];
  name: (id: string | null) => string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <details onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>Previous pairings ({history.length})</summary>
      {open &&
        history.map((p) => (
          <p key={p.id}>
            {name(p.a)} vs {name(p.b)} → {name(p.winner)}
          </p>
        ))}
    </details>
  );
}
function App() {
  const [page, setPage] = useState(location.hash.slice(1) || "arena");
  const [dir, setDir] = useState<Directory | null>(null);
  const [connected, setConnected] = useState(false);
  const [match, setMatch] = useState<Match | null>(null);
  const [selected, setSelected] = useState("");
  const [archive, setArchive] = useState<Summary[]>([]);
  const [archiveOffset, setArchiveOffset] = useState(0);
  const [bots, setBots] = useState<Bot[]>([]);
  const [pages, setPages] = useState({
    roster: 0,
    rankings: 0,
    tournaments: 0,
  });
  const pageRef = useRef(pages);
  const [totals, setTotals] = useState({
    roster: 0,
    rankings: 0,
    tournaments: 0,
  });
  const [tournamentNames, setTournamentNames] = useState<
    Record<string, string>
  >({});
  const [profileBotIds, setProfileBotIds] = useState<Set<string>>(new Set());
  function paginate(kind: keyof typeof pages, offset: number) {
    pageRef.current = { ...pageRef.current, [kind]: offset };
    setPages(pageRef.current);
    refresh();
  }

  const [leaderboard, setLeaderboard] = useState<Bot[]>([]);
  const [profile, setProfile] = useState<Bot | null>(null);
  const [profileHistory, setProfileHistory] = useState<Summary[]>([]);
  const [profileTeam, setProfileTeam] = useState(false);
  const [profileOffset, setProfileOffset] = useState(0);
  async function viewProfile(b: Bot, team = false, offset = 0) {
    try {
      const r = await api<{ matches: Summary[]; bot_ids?: string[] }>(
        `/api/${team ? "teams" : "bots"}/${team ? b.team_id : b.id}/matches?offset=${offset}`,
      );
      setProfile(b);
      setProfileHistory(r.matches);
      setProfileBotIds(new Set(r.bot_ids ?? [b.id]));
      setProfileTeam(team);
      setProfileOffset(offset);
    } catch (e) {
      setError(String(e));
    }
  }
  const [tournaments, setTournaments] = useState<Tournament[]>([]);
  const [standings, setStandings] = useState<Standings[]>([]);
  const [error, setError] = useState("");
  const [clockOffset, setClockOffset] = useState(0);
  const [replayIndex, setReplayIndex] = useState<number | null>(null);
  const [playing, setPlaying] = useState(false);
  const [showIntegrity, setShowIntegrity] = useState(false);
  const [socketGeneration, setSocketGeneration] = useState(0);
  const socket = useRef<WebSocket | null>(null);
  const selectedRef = useRef("");
  const archiveOffsetRef = useRef(0);
  const dirRef = useRef<Directory | null>(null);
  const errors = useRef(0);
  const refreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    try {
      sessionStorage.removeItem("pokerdraft.owner");
    } catch {
      /* Storage may be disabled. */
    }
  }, []);
  const [owner, setOwner] = useState("");
  const [account, setAccount] = useState<{
    team: { id: string; name: string };
    bots: Bot[];
  } | null>(null);
  function navigate(p: string) {
    location.hash = p;
    setPage(p);
    window.scrollTo({ top: 0, behavior: "smooth" });
  }
  const refreshing = useRef(false);
  const refreshAgain = useRef(false);
  async function refresh() {
    if (refreshing.current) {
      refreshAgain.current = true;
      return;
    }
    refreshing.current = true;
    const requested = { ...pageRef.current };
    try {
      const [a, b, t, r] = await Promise.all([
        api<{ matches: Summary[] }>("/api/matches"),
        api<{ bots: Bot[]; total: number }>(
          `/api/bots?offset=${requested.roster}`,
        ),
        api<{
          tournaments: Tournament[];
          standings: Standings[];
          total: number;
          names: Record<string, string>;
        }>(`/api/tournaments?offset=${requested.tournaments}`),
        api<{ bots: Bot[]; total: number }>(
          `/api/leaderboard?offset=${requested.rankings}`,
        ),
      ]);
      if (archiveOffsetRef.current === 0) {
        setArchive(a.matches);
        setArchiveOffset(0);
      }
      if (JSON.stringify(requested) === JSON.stringify(pageRef.current)) {
        setBots(b.bots);
        setTournaments(t.tournaments);
        setStandings(t.standings);
        setLeaderboard(r.bots);
        setTournamentNames(t.names ?? {});
        setTotals({
          roster: b.total ?? b.bots.length,
          rankings: r.total ?? r.bots.length,
          tournaments: t.total ?? t.tournaments.length,
        });
      } else {
        refreshAgain.current = true;
      }
    } catch (e) {
      setError(String(e));
    } finally {
      refreshing.current = false;
      if (refreshAgain.current) {
        refreshAgain.current = false;
        refresh();
      }
    }
  }
  async function loadMatch(id: string, replay = false) {
    try {
      selectedRef.current = id;
      setSelected(id);
      setReplayIndex(null);
      setPlaying(false);
      const m = await api<Match>(
        `/api/matches/${id}${replay ? "/replay" : ""}`,
      );
      if (selectedRef.current !== id) return;
      setMatch(m);
      if (m.status !== "live") setReplayIndex((m.history?.length ?? 1) - 1);
      socket.current?.readyState === WebSocket.OPEN &&
        socket.current.send(
          JSON.stringify({
            op: "watch",
            id: crypto.randomUUID(),
            match_id: id,
          }),
        );
    } catch (e) {
      setError(String(e));
    }
  }
  useEffect(() => {
    const update = () => setPage(location.hash.slice(1) || "arena");
    window.addEventListener("hashchange", update);
    refresh();
    const polling = setInterval(() => {
      if (document.visibilityState === "visible") refresh();
    }, 15000);
    return () => {
      window.removeEventListener("hashchange", update);
      clearInterval(polling);
    };
  }, []);
  useEffect(() => {
    let stopped = false;
    let retry: ReturnType<typeof setTimeout>;
    let heartbeat: ReturnType<typeof setInterval>;
    const connect = () => {
      const ws = new WebSocket(
        `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`,
      );
      socket.current = ws;
      ws.onopen = () => {
        setConnected(true);
        setError("");
        errors.current = 0;
        if (selectedRef.current)
          ws.send(
            JSON.stringify({
              op: "watch",
              id: crypto.randomUUID(),
              match_id: selectedRef.current,
            }),
          );
        heartbeat = setInterval(() => {
          if (ws.readyState === WebSocket.OPEN)
            ws.send(JSON.stringify({ op: "ping", id: crypto.randomUUID() }));
        }, 20000);
      };
      ws.onmessage = (e) => {
        let data;
        try {
          data = JSON.parse(e.data);
        } catch {
          return;
        }
        if (data.server_time_ms)
          setClockOffset(data.server_time_ms - Date.now());
        if (data.type === "directory") {
          setDir(data);
          dirRef.current = data;
          if (!selectedRef.current && data.matches.length)
            loadMatch(data.matches[0].id);
          if (refreshTimer.current) clearTimeout(refreshTimer.current);
          refreshTimer.current = setTimeout(refresh, 300);
        }
        if (data.type === "match" && data.match.id === selectedRef.current) {
          setMatch(data.match);
          if (data.match.server_time_ms)
            setClockOffset(data.match.server_time_ms - Date.now());
          if (data.match.status !== "live") {
            refresh();
            loadMatch(data.match.id, true);
          }
        }
        if (
          data.type === "ack" &&
          data.ok &&
          data.result?.match &&
          data.result.match.id === selectedRef.current
        )
          setMatch((old) => (old?.history ? old : data.result.match));
      };
      ws.onerror = () => ws.close();
      ws.onclose = () => {
        setConnected(false);
        clearInterval(heartbeat);
        if (!stopped)
          retry = setTimeout(
            connect,
            Math.min(1000 * 2 ** errors.current++, 10000),
          );
      };
    };
    connect();
    return () => {
      stopped = true;
      clearTimeout(retry);
      clearInterval(heartbeat);
      if (refreshTimer.current) clearTimeout(refreshTimer.current);
      socket.current?.close();
    };
  }, [socketGeneration]);
  useEffect(() => {
    if (!playing || !match?.history) return;
    const timer = setInterval(
      () =>
        setReplayIndex((i) => {
          const next = (i ?? 0) + 1;
          if (next >= match.history!.length) {
            setPlaying(false);
            return match.history!.length - 1;
          }
          return next;
        }),
      600,
    );
    return () => clearInterval(timer);
  }, [playing, match?.history]);
  useEffect(() => {
    let cancelled = false;
    if (owner)
      command<{ team: { id: string; name: string }; bots: Bot[] }>(
        "account",
        {},
        owner,
      )
        .then((a) => {
          if (!cancelled) setAccount(a);
        })
        .catch(() => {
          if (!cancelled) setAccount(null);
        });
    else setAccount(null);
    return () => {
      cancelled = true;
    };
  }, [owner]);
  const snap: Snapshot | null =
    replayIndex !== null && match?.history
      ? (match.history[replayIndex] ?? match.snapshot)
      : (match?.snapshot ?? null);
  const botById = useMemo(() => new Map(bots.map((b) => [b.id, b])), [bots]);
  const name = (id: string | null) => {
    const b = id ? botById.get(id) : null;
    return b
      ? `${b.team} / ${b.name}`
      : id
        ? (tournamentNames[id] ?? id.slice(0, 8))
        : "Bye";
  };
  return (
    <>
      <a
        className="skip"
        href="#main"
        onClick={(e) => {
          e.preventDefault();
          document.getElementById("main")?.focus();
        }}
      >
        Skip to content
      </a>
      <header>
        <div className="header-inner">
          <a
            className="brand"
            href="#arena"
            onClick={() => navigate("arena")}
            aria-label="Poker Draft home"
          >
            <span className="brand-symbol">♦</span>
            <span>
              POKER<span className="brand-light">DRAFT</span>
            </span>
          </a>
          <nav aria-label="Main navigation">
            {[
              ["arena", "The arena"],
              ["rankings", "Rankings"],
              ["tournaments", "Tournaments"],
              ["docs", "Build a bot"],
            ].map(([id, title]) => (
              <a
                key={id}
                className={page === id ? "active" : ""}
                href={`#${id}`}
                onClick={() => navigate(id)}
              >
                {title}
              </a>
            ))}
          </nav>
          <button
            className="button dark compact"
            onClick={() => navigate("workshop")}
          >
            My workshop <Arrow />
          </button>
          <span
            className={`connection ${connected ? "online" : ""}`}
            title={
              connected
                ? "Live connection established"
                : "Reconnecting to the arena"
            }
          >
            <i />
            {connected ? "LIVE" : "CONNECTING"}
          </span>
        </div>
      </header>
      <main id="main" tabIndex={-1}>
        <ErrorNote message={error} />
        {page === "arena" && (
          <>
            <section className="hero">
              <div>
                <p className="eyebrow">
                  <span className="tiny-diamond">◆</span> THE OPEN ALGORITHMIC
                  CARD ARENA
                </p>
                <h1>
                  Every card has a price.
                  <br />
                  <em>Make yours count.</em>
                </h1>
                <p className="hero-copy">
                  Build a bot. Bid for your hand. Beat the table.
                  <br />A live poker draft where the sharpest strategy wins.
                </p>
                <div className="hero-actions">
                  <button
                    className="button dark"
                    onClick={() => navigate("docs")}
                  >
                    Build your contender <Arrow />
                  </button>
                  <span>52 cards. 6 bots. Endless possibilities.</span>
                </div>
              </div>
              <div className="hero-note">
                <div className="orbit">
                  <span>♠</span>
                  <i>♦</i>
                  <b>♣</b>
                </div>
                <p>
                  CODE YOUR EDGE.
                  <br />
                  LET THE CARDS FALL.
                </p>
                <span>OPEN SOURCE · FREE TO PLAY</span>
              </div>
            </section>
            <section className="arena-section" aria-label="Live match">
              <div className="section-heading">
                <div className="heading-group">
                  <h2>The live table</h2>
                  <Badge green={match?.status === "live"}>
                    {match?.status === "live"
                      ? "● LIVE MATCH"
                      : match?.status === "aborted"
                        ? "ABORTED"
                        : match
                          ? "MATCH REPLAY"
                          : "WAITING FOR PLAY"}
                  </Badge>
                </div>
                <div className="table-select">
                  {(dir?.matches.length ?? 0) > 0 && (
                    <>
                      <label htmlFor="table-select">Watching</label>
                      <select
                        id="table-select"
                        value={selected}
                        onChange={(e) => loadMatch(e.target.value)}
                      >
                        {!dir?.matches.some((m) => m.id === selected) &&
                          selected && (
                            <option value={selected}>
                              Replay · {selected.slice(0, 8)}
                            </option>
                          )}
                        {dir?.matches.map((m, i) => (
                          <option key={m.id} value={m.id}>
                            Table {i + 1} · {m.id.slice(0, 6)}
                            {m.tournament ? " · tournament" : ""}
                          </option>
                        ))}
                      </select>
                    </>
                  )}
                </div>
              </div>
              <div className="arena-layout">
                <div className="felt">
                  <div className="felt-top">
                    <span>
                      POKER DRAFT{" "}
                      <span className="dim">
                        /{" "}
                        {selected
                          ? selected.slice(0, 8).toUpperCase()
                          : "LOBBY"}
                      </span>
                    </span>
                    <span>
                      {snap?.rules.pricing === "first_price"
                        ? "FIRST-PRICE"
                        : "SECOND-PRICE"}{" "}
                      AUCTION <span className="lock">SEALED</span>
                    </span>
                  </div>
                  <div className="auction-floor">
                    <div className="auction-info">
                      <p className="eyebrow light">
                        {snap?.finished
                          ? "THE TABLE IS SETTLED"
                          : "ON THE BLOCK"}
                      </p>
                      <h3>
                        {snap?.finished
                          ? "Final results"
                          : snap?.card != null
                            ? `${RANKS[snap.card % 13]}${SUITS[Math.floor(snap.card / 13)]}`
                            : "The next hand awaits."}
                      </h3>
                      <p>
                        {snap?.finished
                          ? "Review the draft, card by card."
                          : "One card. One sealed bid. Your next move."}
                      </p>
                      <div className="pot">
                        <span>SHOWDOWN POT</span>
                        <strong>
                          {snap?.pot ?? 0}
                          <small> cr</small>
                        </strong>
                      </div>
                    </div>
                    <div
                      className="auction-card"
                      key={`${selected}:${snap?.round}`}
                    >
                      <Card id={snap?.card} />
                      <span className="card-shadow" />
                    </div>
                    <AuctionClock
                      match={match}
                      snap={snap}
                      offset={clockOffset}
                    />
                  </div>
                  <div className="deck-progress">
                    <span>
                      CARD{" "}
                      {Math.min(
                        52,
                        (snap?.round ?? 0) + (snap?.finished ? 0 : 1),
                      )}{" "}
                      <span className="dim">/ 52</span>
                    </span>
                    <div>
                      {Array.from({ length: 52 }, (_, i) => (
                        <i
                          key={i}
                          className={`${i < (snap?.round ?? 0) ? "dealt" : ""} ${i === (snap?.round ?? 0) && !snap?.finished ? "current" : ""} ${i % 13 === 12 ? "quarter" : ""}`}
                        />
                      ))}
                    </div>
                    <span className="dim">♠ ♥ ♦ ♣</span>
                  </div>
                  <div className="seats">
                    {snap?.players.map((p, i) => (
                      <div className="seat" key={p.id}>
                        <div className="seat-header">
                          <span className={`avatar a${i}`}>
                            {p.name.slice(0, 1)}
                          </span>
                          <div>
                            <strong title={p.name}>{p.name}</strong>
                            <span>
                              {p.house
                                ? "HOUSE AGENT"
                                : `CONTENDER ${String(pages.rankings + i + 1).padStart(2, "0")}`}
                            </span>
                          </div>
                          <div className="seat-score">
                            <strong>{p.score}</strong>
                            <span>POINTS</span>
                          </div>
                        </div>
                        <div className="rack">
                          {p.cards.map((c) => (
                            <Card small id={c} key={c} />
                          ))}
                          {Array.from(
                            { length: Math.max(0, 5 - p.cards.length) },
                            (_, j) => (
                              <Card small ghost key={`g${j}`} />
                            ),
                          )}
                        </div>
                        <div className="seat-footer">
                          <span>
                            {p.cards.length}
                            <span className="dim"> / 10 cards</span>
                          </span>
                          <span>
                            ◉ <b>{p.balance}</b>{" "}
                            <span className="dim">credits</span>
                          </span>
                        </div>
                      </div>
                    ))}
                    {!snap && (
                      <div className="empty dark-empty">
                        <h3>The arena is warming up.</h3>
                        <p>
                          {connected
                            ? "Connect a bot to join the next match."
                            : "Connecting to the game server…"}
                        </p>
                        <button
                          className="button lime"
                          onClick={() => navigate("docs")}
                        >
                          Meet the rules <Arrow />
                        </button>
                      </div>
                    )}
                  </div>
                  <div className="felt-bottom">
                    <span>
                      ◆{" "}
                      {snap?.rules.auction_ms ?? dir?.rules.auction_ms ?? 1000}
                      ms windows <span className="dim">·</span> 10-card
                      inventory cap
                    </span>
                    <button
                      onClick={() => setShowIntegrity(!showIntegrity)}
                      aria-expanded={showIntegrity}
                    >
                      Shuffle integrity {showIntegrity ? "−" : "+"}
                    </button>
                  </div>
                  {showIntegrity && snap && (
                    <div className="integrity">
                      <span>SEED COMMITMENT · SHA-256</span>
                      <code>{snap.commitment}</code>
                      <span>
                        {snap.seed
                          ? "REVEALED SEED"
                          : "SEED REVEALED AFTER THE MATCH"}
                      </span>
                      {snap.seed && <code>{snap.seed}</code>}
                      <p>
                        The commitment makes the shuffle auditable after play.
                        The future deck and live bids stay private.
                      </p>
                    </div>
                  )}
                </div>
                <aside className="tape">
                  <div className="tape-title">
                    <h3>Market tape</h3>
                    <span className="pulse-dot" />
                  </div>
                  <p className="muted">Every move leaves a mark.</p>
                  <div className="tape-events" aria-live="off">
                    {(match?.events ?? [])
                      .filter((e) => !snap || e.seq <= snap.seq)
                      .slice()
                      .sort((a, b) => b.seq - a.seq)
                      .slice(0, 12)
                      .map((e) => (
                        <TapeEvent key={e.seq} event={e} snapshot={snap} />
                      ))}
                    {!match?.events?.length && (
                      <p className="empty-note">
                        The first card starts the story.
                      </p>
                    )}
                  </div>
                  <div className="lobby-note">
                    <span className="eyebrow">NEXT UP</span>
                    <strong>
                      {dir?.lobby.bots.length ?? 0} / {dir?.rules.seats ?? 6}{" "}
                      contenders
                    </strong>
                    <LobbyCountdown
                      deadline={dir?.lobby.deadline_ms}
                      offset={clockOffset}
                      paused={dir?.paused}
                    />
                    <button
                      className="text-link"
                      onClick={() => navigate("workshop")}
                    >
                      Get your bot in the game <Arrow />
                    </button>
                  </div>
                </aside>
              </div>
              {match?.history && (
                <div className="replay-controls">
                  <button
                    className="button compact"
                    onClick={() => {
                      if ((replayIndex ?? 0) >= match.history!.length - 1)
                        setReplayIndex(0);
                      setPlaying(!playing);
                    }}
                  >
                    {playing ? "Pause replay" : "Play replay"}
                  </button>
                  <label htmlFor="replay-range">Draft timeline</label>
                  <input
                    id="replay-range"
                    type="range"
                    min="0"
                    max={match.history.length - 1}
                    value={replayIndex ?? 0}
                    onChange={(e) => {
                      setPlaying(false);
                      setReplayIndex(Number(e.target.value));
                    }}
                  />
                  <span>
                    {(replayIndex ?? 0) + 1} / {match.history.length}
                  </span>
                  <a
                    className="text-link"
                    href={`/api/matches/${match.id}/replay`}
                    download={`pokerdraft-${match.id}.json`}
                  >
                    Download replay ↓
                  </a>
                </div>
              )}
            </section>
            <section className="lower-grid">
              <div>
                <div className="section-heading">
                  <h2>Recent drafts</h2>
                  <span className="eyebrow">THE RECORD NEVER FOLDS</span>
                </div>
                <div className="data-table">
                  <table>
                    <thead>
                      <tr>
                        <th>Match</th>
                        <th>Table</th>
                        <th>Result</th>
                        <th>
                          <span className="sr-only">Watch match</span>
                        </th>
                      </tr>
                    </thead>
                    <tbody>
                      {archive.map((m) => (
                        <tr key={m.id}>
                          <td>
                            <strong>#{m.id.slice(0, 6).toUpperCase()}</strong>
                            <small>
                              {m.created_ms
                                ? new Date(m.created_ms).toLocaleTimeString(
                                    [],
                                    { hour: "2-digit", minute: "2-digit" },
                                  )
                                : ""}
                            </small>
                          </td>
                          <td>
                            {m.players.filter((p) => !p.house).length}{" "}
                            contenders{" "}
                            <small>
                              {m.players.length} seats
                              {m.tournament ? " · tournament" : ""}
                            </small>
                          </td>
                          <td>
                            <Badge green={m.status === "live"}>
                              {m.status === "live"
                                ? `LIVE · ${m.round}/52`
                                : m.status?.toUpperCase()}
                            </Badge>
                          </td>
                          <td>
                            <button
                              className="round-button"
                              aria-label={`Watch match ${m.id.slice(0, 6)}`}
                              onClick={() => loadMatch(m.id, true)}
                            >
                              <Arrow />
                            </button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  {!archive.length && (
                    <p className="empty-note">
                      Completed drafts and live matches will appear here.
                    </p>
                  )}
                </div>
                <button
                  className="text-link archive-more"
                  disabled={archive.length < 50}
                  onClick={async () => {
                    try {
                      const offset = archiveOffset + 50;
                      const r = await api<{ matches: Summary[] }>(
                        `/api/matches?offset=${offset}`,
                      );
                      setArchive(r.matches);
                      setArchiveOffset(offset);
                      archiveOffsetRef.current = offset;
                    } catch (e) {
                      setError(String(e));
                    }
                  }}
                >
                  Older drafts →
                </button>
                {archiveOffset > 0 && (
                  <button
                    className="text-link"
                    onClick={() => {
                      archiveOffsetRef.current = 0;
                      setArchiveOffset(0);
                      refresh();
                    }}
                  >
                    Back to latest
                  </button>
                )}
              </div>
              <div className="build-banner">
                <span className="eyebrow">YOUR STRATEGY. YOUR BOT.</span>
                <h3>
                  The next seat
                  <br />
                  could be yours.
                </h3>
                <p>
                  Bring your own algorithm. Start with a reference bot and find
                  your edge.
                </p>
                <button
                  className="button dark"
                  onClick={() => navigate("docs")}
                >
                  Read the bot guide <Arrow />
                </button>
                <span className="banner-suit">♠</span>
              </div>
            </section>
          </>
        )}
        {page === "rankings" && (
          <section className="page-section">
            <p className="eyebrow">STRATEGY, MEASURED</p>
            <h1>The leaderboard.</h1>
            <p className="intro">
              Multiplayer Elo. Fresh versions start at 1500. Only matches with
              at least two real competitors affect ratings.
            </p>
            <div className="data-table">
              <table>
                <thead>
                  <tr>
                    <th>Rank</th>
                    <th>Contender</th>
                    <th>Team</th>
                    <th>Elo</th>
                    <th>Rated games</th>
                  </tr>
                </thead>
                <tbody>
                  {leaderboard.map((b, i) => (
                    <tr key={b.id}>
                      <td className="rank">
                        {String(pages.rankings + i + 1).padStart(2, "0")}
                      </td>
                      <td>
                        <button
                          className="profile-link"
                          onClick={() => viewProfile(b)}
                        >
                          {b.name}
                        </button>
                      </td>
                      <td>{b.team}</td>
                      <td>
                        <strong>{b.rating.toFixed(1)}</strong>
                      </td>
                      <td>{b.games}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {!leaderboard.length && (
                <div className="empty">
                  <h3>A clean slate.</h3>
                  <p>
                    The first rated match needs two real contenders. House
                    agents never rank.
                  </p>
                  <button
                    className="button dark"
                    onClick={() => navigate("workshop")}
                  >
                    Create a contender <Arrow />
                  </button>
                </div>
              )}
            </div>
            <PageControls
              label="rankings"
              offset={pages.rankings}
              total={totals.rankings}
              size={100}
              onChange={(n) => paginate("rankings", n)}
            />
            {profile && (
              <section
                className="profile-panel spaced"
                aria-label="Contender history"
              >
                <div className="section-heading">
                  <div>
                    <p className="eyebrow">{profile.team}</p>
                    <h2>
                      {profile.name} · {profile.rating.toFixed(0)} Elo
                    </h2>
                  </div>
                  <button
                    className="text-link"
                    onClick={() => setProfile(null)}
                  >
                    Close history ×
                  </button>
                </div>
                <div className="action-row">
                  <button
                    className={`button compact ${!profileTeam ? "dark" : ""}`}
                    onClick={() => viewProfile(profile)}
                  >
                    This version
                  </button>
                  <button
                    className={`button compact ${profileTeam ? "dark" : ""}`}
                    onClick={() => viewProfile(profile, true)}
                  >
                    All team versions
                  </button>
                </div>
                <div className="data-table spaced">
                  <table>
                    <thead>
                      <tr>
                        <th>Match</th>
                        <th>Status</th>
                        <th>Earned score</th>
                        <th>Replay</th>
                      </tr>
                    </thead>
                    <tbody>
                      {profileHistory.map((m) => (
                        <tr key={m.id}>
                          <td>
                            #{m.id.slice(0, 8)}
                            <small>
                              {m.created_ms
                                ? new Date(m.created_ms).toLocaleString()
                                : ""}
                            </small>
                          </td>
                          <td>{m.status}</td>
                          <td>
                            {m.players
                              .filter((p) =>
                                profileTeam
                                  ? profileBotIds.has(p.id)
                                  : p.id === profile.id,
                              )
                              .map((p) => (
                                <span key={p.id}>
                                  {p.name} · {p.score} points
                                </span>
                              ))}
                          </td>
                          <td>
                            <button
                              className="text-link"
                              onClick={() => {
                                loadMatch(m.id, true);
                                navigate("arena");
                              }}
                            >
                              Watch <Arrow />
                            </button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  {!profileHistory.length && (
                    <p className="empty-note">No matches recorded yet.</p>
                  )}
                </div>
                <div className="action-row spaced">
                  {profileOffset > 0 && (
                    <button
                      className="text-link"
                      onClick={() =>
                        viewProfile(
                          profile,
                          profileTeam,
                          Math.max(0, profileOffset - 50),
                        )
                      }
                    >
                      ← Newer
                    </button>
                  )}
                  {profileHistory.length === 50 && (
                    <button
                      className="text-link"
                      onClick={() =>
                        viewProfile(profile, profileTeam, profileOffset + 50)
                      }
                    >
                      Older →
                    </button>
                  )}
                </div>
              </section>
            )}
            <h2 className="spaced">Registered contenders</h2>
            <PageControls
              label="roster"
              offset={pages.roster}
              total={totals.roster}
              size={100}
              onChange={(n) => paginate("roster", n)}
            />
            <div className="contender-grid">
              {bots.map((b) => (
                <article className="contender" key={b.id}>
                  <Badge>
                    {b.disabled ? "DISABLED" : b.games ? "RATED" : "UNRATED"}
                  </Badge>
                  <h3>
                    <button
                      className="profile-link"
                      onClick={() => viewProfile(b)}
                    >
                      {b.name}
                    </button>
                  </h3>
                  <p>{b.team}</p>
                  <span>
                    {b.rating.toFixed(0)} Elo · {b.games} games
                  </span>
                </article>
              ))}
              {!bots.length && (
                <p className="muted">No contenders registered yet.</p>
              )}
            </div>
          </section>
        )}
        {page === "tournaments" && (
          <section className="page-section">
            <p className="eyebrow">THE LONG GAME</p>
            <h1>Play for the table.</h1>
            <p className="intro">
              Swiss rounds. A top cut. A best-of-three final. Register a bot
              version from your workshop to enter.
            </p>
            <PageControls
              label="tournaments"
              offset={pages.tournaments}
              total={totals.tournaments}
              size={10}
              onChange={(n) => paginate("tournaments", n)}
            />
            <div className="tournament-list">
              {tournaments.map((t) => (
                <article className="tournament" key={t.id}>
                  <div className="section-heading">
                    <div>
                      <Badge green={t.state === "registration"}>
                        {t.state.replaceAll("_", " ").toUpperCase()}
                      </Badge>
                      <h2>{t.name}</h2>
                    </div>
                    <span className="muted">
                      {t.swiss_rounds} Swiss rounds · Top {t.cut} ·{" "}
                      {t.entrants.length} entrants
                    </span>
                  </div>
                  {t.state === "attention" && (
                    <p role="status" className="error">
                      This tournament is paused after three consecutive drawn
                      games. The operator can review the replays and cancel it.
                      No winner has been assigned.
                    </p>
                  )}
                  {t.champion && (
                    <div className="champion">
                      ♔ Champion: <strong>{name(t.champion)}</strong>
                    </div>
                  )}
                  {t.state === "registration" && (
                    <div className="register-row">
                      {account ? (
                        <>
                          <label htmlFor={`entry-${t.id}`}>Your version</label>
                          <select id={`entry-${t.id}`} defaultValue="">
                            <option value="" disabled>
                              Select a contender
                            </option>
                            {account.bots
                              .filter((b) => !b.disabled)
                              .map((b) => (
                                <option key={b.id} value={b.id}>
                                  {b.name}
                                </option>
                              ))}
                          </select>
                          <button
                            className="button dark compact"
                            onClick={async () => {
                              const field = document.getElementById(
                                `entry-${t.id}`,
                              ) as HTMLSelectElement;
                              try {
                                await command(
                                  "register_tournament",
                                  { tournament_id: t.id, bot_id: field.value },
                                  owner,
                                );
                                refresh();
                              } catch (e) {
                                setError(String(e));
                              }
                            }}
                          >
                            Register <Arrow />
                          </button>
                          <button
                            className="text-link"
                            onClick={async () => {
                              const field = document.getElementById(
                                `entry-${t.id}`,
                              ) as HTMLSelectElement;
                              try {
                                await command(
                                  "unregister_tournament",
                                  { tournament_id: t.id, bot_id: field.value },
                                  owner,
                                );
                                refresh();
                              } catch (e) {
                                setError(String(e));
                              }
                            }}
                          >
                            Withdraw
                          </button>
                        </>
                      ) : (
                        <button
                          className="button dark compact"
                          onClick={() => navigate("workshop")}
                        >
                          Open your workshop <Arrow />
                        </button>
                      )}
                    </div>
                  )}
                  <div className="tournament-columns">
                    <div>
                      <h3>Standings</h3>
                      <div className="data-table">
                        <table>
                          <thead>
                            <tr>
                              <th>Contender</th>
                              <th>Points</th>
                              <th>Buchholz</th>
                            </tr>
                          </thead>
                          <tbody>
                            {standings
                              .find((s) => s.id === t.id)
                              ?.entries.map((e) => (
                                <tr key={e.bot}>
                                  <td>
                                    {name(e.bot)}
                                    {e.byes > 0 && <small>{e.byes} bye</small>}
                                  </td>
                                  <td>{e.points}</td>
                                  <td>{e.buchholz}</td>
                                </tr>
                              ))}
                          </tbody>
                        </table>
                      </div>
                    </div>
                    <div>
                      <h3>
                        Round {t.round || "—"} ·{" "}
                        {t.state === "final"
                          ? "Final series"
                          : t.state === "elimination"
                            ? "Knockout"
                            : "Pairings"}
                      </h3>
                      {t.pairings.map((p) => (
                        <div className="pairing" key={p.id}>
                          <span className={p.winner === p.a ? "winner" : ""}>
                            {name(p.a)}
                          </span>
                          <span className="versus">{p.wins.join(" : ")}</span>
                          <span className={p.winner === p.b ? "winner" : ""}>
                            {name(p.b)}
                          </span>
                          {p.match_id && (
                            <button
                              className="text-link"
                              onClick={() => {
                                loadMatch(p.match_id!);
                                navigate("arena");
                              }}
                            >
                              Watch ↗
                            </button>
                          )}
                          {p.winner && (
                            <small>
                              {p.b
                                ? `${name(p.winner)} advances`
                                : "Bye · 3 points"}
                            </small>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                  {t.history.length > 0 && (
                    <PreviousPairings history={t.history} name={name} />
                  )}
                </article>
              ))}
              {!tournaments.length && (
                <div className="empty bordered">
                  <span className="empty-symbol">♔</span>
                  <h3>The next cup is coming.</h3>
                  <p>
                    No tournaments have been scheduled. The rolling arena is
                    always open.
                  </p>
                  <button
                    className="button dark"
                    onClick={() => navigate("arena")}
                  >
                    Watch the arena <Arrow />
                  </button>
                </div>
              )}
            </div>
          </section>
        )}
        {page === "workshop" && (
          <Workshop
            owner={owner}
            setOwner={setOwner}
            account={account}
            setAccount={setAccount}
            refresh={refresh}
            navigate={navigate}
          />
        )}
        {page === "docs" && <Docs navigate={navigate} />}
        {page === "operator" && (
          <Operator
            tournaments={tournaments}
            matches={dir?.matches ?? []}
            bots={bots}
            refresh={refresh}
            rosterPager={
              <PageControls
                label="operator roster"
                offset={pages.roster}
                total={totals.roster}
                size={100}
                onChange={(n) => paginate("roster", n)}
              />
            }
            tournamentPager={
              <PageControls
                label="operator tournaments"
                offset={pages.tournaments}
                total={totals.tournaments}
                size={10}
                onChange={(n) => paginate("tournaments", n)}
              />
            }
          />
        )}
        {![
          "arena",
          "rankings",
          "tournaments",
          "workshop",
          "docs",
          "operator",
        ].includes(page) && (
          <section className="empty">
            <h1>Table not found.</h1>
            <button className="button dark" onClick={() => navigate("arena")}>
              Back to the arena
            </button>
          </section>
        )}
      </main>
      <footer>
        <a className="brand" href="#arena" onClick={() => navigate("arena")}>
          <span className="brand-symbol">♦</span>
          <span>
            POKER<span className="brand-light">DRAFT</span>
          </span>
        </a>
        <p>Built for the love of the game. And the algorithm.</p>
        <div>
          <a
            href="https://github.com/Owen-Isenhart/PokerDraft"
            target="_blank"
            rel="noreferrer"
          >
            Source code
          </a>
          <a href="#operator" onClick={() => navigate("operator")}>
            Operator
          </a>
          <span>FICTIONAL CREDITS · NO REAL MONEY</span>
        </div>
      </footer>
    </>
  );
}
function TapeEvent({
  event: e,
  snapshot: s,
}: {
  event: GameEvent;
  snapshot: Snapshot | null;
}) {
  const d = e.data;
  const seat = typeof d.seat === "number" ? d.seat : null;
  const name = seat !== null ? s?.players[seat]?.name : "Table";
  let text = e.kind.replaceAll("_", " ");
  let icon = "♦";
  if (e.kind === "award") {
    text = `${typeof d.card === "number" ? RANKS[d.card % 13] + SUITS[Math.floor(d.card / 13)] : ""} won for ${d.price} credits`;
    icon = "♠";
  }
  if (e.kind === "cash_in") {
    text = `Cashed ${d.name ?? "a hand"}`;
    icon = "◉";
  }
  if (e.kind === "showdown") {
    text = "Showdown settled";
    icon = "♔";
  }
  if (e.kind === "burn") {
    text = "No bids · card burned";
    icon = "×";
  }
  if (e.kind === "finished") text = "Final scores recorded";
  if (e.kind === "started") text = "A fresh deck hits the table";
  if (e.kind === "aborted") text = `Match aborted · ${String(d.reason ?? "")}`;
  return (
    <div className="tape-event">
      <span className="event-icon">{icon}</span>
      <div>
        <strong>{name}</strong>
        <p>{text}</p>
        <span>
          CARD{" "}
          {Math.min(
            52,
            e.round + (["award", "burn", "started"].includes(e.kind) ? 1 : 0),
          )
            .toString()
            .padStart(2, "0")}{" "}
          <span>· #{e.seq}</span>
        </span>
      </div>
    </div>
  );
}
function Workshop({
  owner,
  setOwner,
  account,
  setAccount,
  refresh,
  navigate,
}: {
  owner: string;
  setOwner: (v: string) => void;
  account: { team: { id: string; name: string }; bots: Bot[] } | null;
  setAccount: (
    v: { team: { id: string; name: string }; bots: Bot[] } | null,
  ) => void;
  refresh: () => void;
  navigate: (p: string) => void;
}) {
  const [teamName, setTeamName] = useState("");
  const [version, setVersion] = useState("");
  const [tokenInput, setTokenInput] = useState(owner);
  const [secret, setSecret] = useState<{ label: string; value: string } | null>(
    null,
  );
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const [copied, setCopied] = useState(false);
  async function task(fn: () => Promise<void>) {
    setPending(true);
    setError("");
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    } finally {
      setPending(false);
    }
  }
  async function load(token = owner) {
    const a = await command<{
      team: { id: string; name: string };
      bots: Bot[];
    }>("account", {}, token);
    setAccount(a);
  }
  return (
    <section className="page-section">
      <p className="eyebrow">THE WORKSHOP</p>
      <h1>Build your edge.</h1>
      <p className="intro">
        One team. Your bot versions. A new strategy whenever inspiration
        strikes.
      </p>
      <ErrorNote message={error} />
      {secret && (
        <div className="secret-panel" role="status">
          <h3>{secret.label}</h3>
          <p>
            Copy and store this credential now. It is shown only when created.
            Anyone with it can act for your{" "}
            {secret.label.includes("Owner") ? "team" : "bot"}.
          </p>
          <code>{secret.value}</code>
          <button
            className="button compact"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(secret.value);
                setCopied(true);
              } catch {
                setError(
                  "Clipboard unavailable. Select and copy the credential.",
                );
              }
            }}
          >
            {copied ? "Copied" : "Copy credential"}
          </button>
          <button
            className="text-link"
            onClick={() => {
              setSecret(null);
              setCopied(false);
            }}
          >
            I saved it · dismiss
          </button>
        </div>
      )}
      {!account ? (
        <div className="workshop-grid">
          <form
            className="form-panel"
            onSubmit={(e) => {
              e.preventDefault();
              task(async () => {
                const r = await command<{
                  team: { id: string; name: string };
                  owner_token: string;
                }>("create_team", { name: teamName });
                setOwner(r.owner_token);
                setTokenInput(r.owner_token);
                setSecret({ label: "Owner credential", value: r.owner_token });
                await load(r.owner_token);
                refresh();
              });
            }}
          >
            <span className="step-number">01</span>
            <h2>Make a name.</h2>
            <p>Your team is the identity behind every contender you build.</p>
            <label htmlFor="team-name">Team name</label>
            <input
              id="team-name"
              value={teamName}
              minLength={2}
              maxLength={40}
              required
              onChange={(e) => setTeamName(e.target.value)}
              placeholder="e.g. Full House Labs"
            />
            <button className="button dark" disabled={pending}>
              Create your team <Arrow />
            </button>
          </form>
          <form
            className="form-panel"
            onSubmit={(e) => {
              e.preventDefault();
              task(async () => {
                await load(tokenInput);
                setOwner(tokenInput);
              });
            }}
          >
            <span className="step-number">↩</span>
            <h2>Back at the bench?</h2>
            <p>
              Restore your workshop with your owner credential. It stays in
              memory until you lock or reload the workshop.
            </p>
            <label htmlFor="owner-credential">Owner credential</label>
            <input
              id="owner-credential"
              type="password"
              autoComplete="off"
              value={tokenInput}
              required
              onChange={(e) => setTokenInput(e.target.value)}
              placeholder="Paste your owner credential"
            />
            <button className="button" disabled={pending}>
              Open workshop <Arrow />
            </button>
          </form>
        </div>
      ) : (
        <>
          <div className="section-heading">
            <div>
              <Badge green>TEAM WORKSHOP</Badge>
              <h2>{account.team.name}</h2>
            </div>
            <button
              className="text-link"
              onClick={() => {
                setOwner("");
                setAccount(null);
                setTokenInput("");
                setSecret(null);
              }}
            >
              Lock workshop ↗
            </button>
          </div>
          <div className="workshop-grid">
            <form
              className="form-panel"
              onSubmit={(e) => {
                e.preventDefault();
                task(async () => {
                  const r = await command<{ bot: Bot; bot_token: string }>(
                    "create_bot",
                    { name: version },
                    owner,
                  );
                  setSecret({ label: "Bot credential", value: r.bot_token });
                  setCopied(false);
                  setVersion("");
                  await load();
                  refresh();
                });
              }}
            >
              <span className="step-number">02</span>
              <h2>Meet your contender.</h2>
              <p>
                Each named version gets its own credential and starts at 1500
                Elo. Keep old versions to compare strategies.
              </p>
              <label htmlFor="version-name">Version name</label>
              <input
                id="version-name"
                value={version}
                minLength={2}
                maxLength={40}
                required
                onChange={(e) => setVersion(e.target.value)}
                placeholder="e.g. river-v1"
              />
              <button className="button dark" disabled={pending}>
                Create bot version <Arrow />
              </button>
            </form>
            <div className="form-panel">
              <span className="step-number">03</span>
              <h2>Bring it to life.</h2>
              <p>
                Run your bot on your computer. Give it a bot credential, connect
                to this arena, and join the rolling lobby.
              </p>
              <pre>
                <code>
                  export POKERDRAFT_URL={location.origin}
                  {"\n"}export POKERDRAFT_TOKEN=&lt;bot-credential&gt;{"\n"}
                  python3 sdk/python/bot.py
                </code>
              </pre>
              <button className="text-link" onClick={() => navigate("docs")}>
                Open the complete bot guide <Arrow />
              </button>
            </div>
          </div>
          <h2 className="spaced">Your versions</h2>
          <div className="data-table">
            <table>
              <thead>
                <tr>
                  <th>Version</th>
                  <th>Elo</th>
                  <th>Rated games</th>
                  <th>Credentials</th>
                </tr>
              </thead>
              <tbody>
                {account.bots.map((b) => (
                  <tr key={b.id}>
                    <td>
                      <strong>{b.name}</strong>
                      <small>{b.id}</small>
                    </td>
                    <td>{b.rating.toFixed(1)}</td>
                    <td>{b.games}</td>
                    <td>
                      <button
                        className="text-link"
                        disabled={pending}
                        onClick={() => {
                          if (
                            !confirm(
                              `Rotate ${b.name}'s credential? Its previous credential will stop working immediately.`,
                            )
                          )
                            return;
                          task(async () => {
                            const r = await command<{ bot_token: string }>(
                              "rotate_bot",
                              { bot_id: b.id },
                              owner,
                            );
                            setSecret({
                              label: "New bot credential",
                              value: r.bot_token,
                            });
                            setCopied(false);
                          });
                        }}
                      >
                        Rotate token ↻
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {!account.bots.length && (
              <p className="empty-note">Create your first bot version above.</p>
            )}
          </div>
          <details className="spaced">
            <summary>Owner credential settings</summary>
            <p>
              Rotate the owner credential if it has been exposed. Previous
              access stops immediately.
            </p>
            <button
              className="button compact"
              disabled={pending}
              onClick={() => {
                if (
                  !confirm(
                    "Rotate your owner credential? Store the new one to keep access.",
                  )
                )
                  return;
                task(async () => {
                  const r = await command<{ owner_token: string }>(
                    "rotate_owner",
                    {},
                    owner,
                  );
                  setOwner(r.owner_token);
                  setSecret({
                    label: "New Owner credential",
                    value: r.owner_token,
                  });
                  setCopied(false);
                });
              }}
            >
              Rotate owner credential
            </button>
          </details>
        </>
      )}
    </section>
  );
}
function Docs({ navigate }: { navigate: (p: string) => void }) {
  return (
    <section className="page-section docs">
      <p className="eyebrow">THE BOT BUILDER'S FIELD GUIDE</p>
      <h1>
        Turn code into
        <br />
        <em>a contender.</em>
      </h1>
      <p className="intro">
        No AI subscription. No model required. Bring a heuristic, a search
        algorithm, or a learning agent. The table only cares about your next
        move.
      </p>
      <div className="docs-layout">
        <aside>
          <a
            href="#quickstart"
            onClick={(e) => {
              e.preventDefault();
              document
                .getElementById("quickstart")
                ?.scrollIntoView({ behavior: "smooth" });
            }}
          >
            01 · Quick start
          </a>
          <a
            href="#mechanics"
            onClick={(e) => {
              e.preventDefault();
              document
                .getElementById("mechanics")
                ?.scrollIntoView({ behavior: "smooth" });
            }}
          >
            02 · The game
          </a>
          <a
            href="#protocol"
            onClick={(e) => {
              e.preventDefault();
              document
                .getElementById("protocol")
                ?.scrollIntoView({ behavior: "smooth" });
            }}
          >
            03 · The protocol
          </a>
          <a
            href="#economy"
            onClick={(e) => {
              e.preventDefault();
              document
                .getElementById("economy")
                ?.scrollIntoView({ behavior: "smooth" });
            }}
          >
            04 · The economy
          </a>
          <a
            href="#fairness"
            onClick={(e) => {
              e.preventDefault();
              document
                .getElementById("fairness")
                ?.scrollIntoView({ behavior: "smooth" });
            }}
          >
            05 · Fair play
          </a>
        </aside>
        <div>
          <section id="quickstart">
            <h2>01 / Take your seat</h2>
            <p>
              Create a team and bot version in your workshop. Save the owner
              credential for team management and use the bot credential only in
              your agent.
            </p>
            <button
              className="button dark compact"
              onClick={() => navigate("workshop")}
            >
              Open workshop <Arrow />
            </button>
            <p>
              Clone the repository, then run either starter. Python uses only
              the standard library; TypeScript uses Node 22 or later.
            </p>
            <pre>
              <code>
                git clone https://github.com/Owen-Isenhart/PokerDraft{"\n"}cd
                PokerDraft{"\n"}git checkout work{"\n"}export POKERDRAFT_URL=
                {location.origin}
                {"\n"}export POKERDRAFT_TOKEN=&lt;your-bot-credential&gt;{"\n"}
                python3 sdk/python/bot.py{"\n"}# or: node
                --experimental-strip-types sdk/typescript/bot.ts
              </code>
            </pre>
            <p>
              The starters authenticate, join the queue, reconnect after
              disconnects, and rejoin after each completed arena match. In a
              registered tournament they wait for assigned matches. Edit the
              bidding and cash-in strategy, keeping the network lifecycle
              intact.
            </p>
          </section>
          <section id="mechanics">
            <h2>02 / Know the table</h2>
            <p>
              Start with 100 credits. Each card appears once in a 52-card draft.
              One immutable sealed bid per player per card. Highest bid wins;
              the winner pays the second highest bid, with a one-credit reserve.
              An auction with no bids burns the card.
            </p>
            <p>
              Your inventory holds at most ten cards. During bidding, cash in
              any five owned cards for credits and score. Every thirteen cards,
              a showdown automatically compares each qualifying player's best
              five cards. All entered hands are consumed; the strongest hand
              takes the pot. Exact ties split it.
            </p>
            <p>
              Every showdown adds 40 credits to the pot, alongside auction
              payments. If no player has five cards, the pot rolls over. After
              card 52, leftover five-card hands are liquidated automatically.
              Highest earned score wins; spending credits does not subtract
              score.
            </p>
          </section>
          <section id="protocol">
            <h2>03 / Speak the language</h2>
            <p>
              Connect a WebSocket to{" "}
              <code>{location.origin.replace(/^http/, "ws")}/ws</code>. Send
              JSON messages with a unique request ID. Authenticate in the first
              message; keep credentials out of URLs.
            </p>
            <pre>
              <code>
                {JSON.stringify(
                  { op: "auth", id: "unique-1", token: "<bot-credential>" },
                  null,
                  2,
                )}
                {"\n"}
                {JSON.stringify({ op: "join", id: "unique-2" }, null, 2)}
                {"\n"}
                {JSON.stringify(
                  {
                    op: "bid",
                    id: "unique-3",
                    match_id: "<match-id>",
                    round: 0,
                    amount: 7,
                  },
                  null,
                  2,
                )}
                {"\n"}
                {JSON.stringify(
                  {
                    op: "cash",
                    id: "unique-4",
                    match_id: "<match-id>",
                    round: 0,
                    cards: [0, 1, 2, 3, 4],
                  },
                  null,
                  2,
                )}
              </code>
            </pre>
            <p>
              Rounds are zero-based. Card IDs are suit × 13 + rank index: clubs,
              diamonds, hearts, spades; 2 through ace. A missing bid or zero is
              a pass. Integer amounts only, no more than your current balance.
            </p>
            <p>
              Messages arrive as <code>directory</code>, <code>match</code>,{" "}
              <code>ack</code>, and <code>heartbeat</code>. Match messages carry
              public snapshots and the deadline in server milliseconds. Send{" "}
              <code>ping</code> periodically. Reconnect, authenticate, then
              watch the active match from the auth response to recover a
              complete snapshot.
            </p>
            <p>
              Retries must preserve the request ID and payload. Accepted bids
              cannot be changed. Send decisions early: the server rejects
              actions admitted at or after its monotonic deadline. The window
              does not compensate for your network's travel time.
            </p>
            <a
              className="text-link"
              href="https://github.com/Owen-Isenhart/PokerDraft/blob/work/docs/PROTOCOL.md"
              target="_blank"
              rel="noreferrer"
            >
              Complete protocol and error reference
            </a>
          </section>
          <section id="economy">
            <h2>04 / Price the possibilities</h2>
            <p>
              Every cash-in adds its reward to both your spendable balance and
              earned score. Showdown rewards do the same. Each card can only be
              consumed once.
            </p>
            <div className="data-table">
              <table>
                <thead>
                  <tr>
                    <th>Poker hand</th>
                    <th>Reward</th>
                  </tr>
                </thead>
                <tbody>
                  {REWARDS.map(([category, reward]) => (
                    <tr key={category}>
                      <td>{category}</td>
                      <td>
                        <strong>{reward}</strong> credits
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p>
              Ace is high, except in the five-high straight A–2–3–4–5. Standard
              poker kickers break ties; suit never does.
            </p>
          </section>
          <section id="fairness">
            <h2>05 / Play a fair game</h2>
            <p>
              Inventories, winners, balances and clearing prices are public.
              Current bids and the future deck are private. The server commits
              to a seed before play and reveals it after completion or abort.
              Deterministic tie-breaking ignores bid arrival order.
            </p>
            <p>
              The server operator is trusted. A shuffle commitment is an audit
              aid, not proof of a trustless auction. Network latency still
              matters; longer bidding windows can be configured for tournaments.
            </p>
            <p>
              One active version per team. Disconnects pass rather than pause.
              Interrupted server matches are visibly aborted and never rated.
              All points are fictional and cannot be bought, redeemed or
              withdrawn.
            </p>
            <a
              className="text-link"
              href="https://github.com/Owen-Isenhart/PokerDraft/blob/work/docs/RULES.md"
              target="_blank"
              rel="noreferrer"
            >
              Read the exact rules
            </a>
          </section>
        </div>
      </div>
    </section>
  );
}
function Operator({
  tournaments,
  matches,
  bots,
  refresh,
  rosterPager,
  tournamentPager,
}: {
  tournaments: Tournament[];
  matches: Summary[];
  bots: Bot[];
  refresh: () => void;
  rosterPager: React.ReactNode;
  tournamentPager: React.ReactNode;
}) {
  const [token, setToken] = useState("");
  const [name, setName] = useState("");
  const [rounds, setRounds] = useState(3);
  const [cut, setCut] = useState(4);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [pending, setPending] = useState(false);
  async function act(op: string, args: Record<string, unknown>) {
    setPending(true);
    setError("");
    setNotice("");
    try {
      await command(op, args, token);
      setNotice("Saved.");
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setPending(false);
    }
  }
  return (
    <section className="page-section">
      <p className="eyebrow">OPERATOR CONSOLE</p>
      <h1>Run the table.</h1>
      <p className="intro">
        Operator access is enforced by the server. Keep this credential separate
        from team and bot credentials.
      </p>
      <div className="form-panel">
        <label htmlFor="admin-token">Operator credential</label>
        <input
          id="admin-token"
          type="password"
          autoComplete="off"
          value={token}
          onChange={(e) => setToken(e.target.value)}
        />
        <ErrorNote message={error} />
        {notice && <p role="status">{notice}</p>}
        <div className="action-row">
          <button
            className="button compact"
            disabled={pending}
            onClick={() => act("admin_pause", { paused: true })}
          >
            Pause new matches
          </button>
          <button
            className="button compact"
            disabled={pending}
            onClick={() => act("admin_pause", { paused: false })}
          >
            Resume new matches
          </button>
        </div>
      </div>
      <form
        className="form-panel spaced"
        onSubmit={(e) => {
          e.preventDefault();
          act("admin_create_tournament", { name, swiss_rounds: rounds, cut });
        }}
      >
        <h2>Create a tournament</h2>
        <label htmlFor="cup-name">Tournament name</label>
        <input
          id="cup-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          minLength={2}
          maxLength={40}
          required
        />
        <div className="form-row">
          <label>
            Swiss rounds
            <input
              type="number"
              min={1}
              max={12}
              value={rounds}
              onChange={(e) => setRounds(Number(e.target.value))}
            />
          </label>
          <label>
            Top cut
            <select
              value={cut}
              onChange={(e) => setCut(Number(e.target.value))}
            >
              {[2, 4, 8, 16, 32].map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
        </div>
        <button className="button dark" disabled={pending}>
          Open registration <Arrow />
        </button>
      </form>
      <h2 className="spaced">Tournaments</h2>
      {tournamentPager}
      {tournaments.map((t) => (
        <div className="operator-row" key={t.id}>
          <span>
            {t.name} · {t.state} · {t.entrants.length} entrants
          </span>
          <div>
            {t.state === "registration" && (
              <button
                className="button compact"
                disabled={pending}
                onClick={() =>
                  act("admin_start_tournament", { tournament_id: t.id })
                }
              >
                Start
              </button>
            )}
            {!["complete", "cancelled"].includes(t.state) && (
              <button
                className="text-link"
                disabled={pending}
                onClick={() => {
                  if (
                    confirm(
                      `Cancel ${t.name}? Active pairings will be aborted.`,
                    )
                  )
                    act("admin_cancel_tournament", { tournament_id: t.id });
                }}
              >
                Cancel
              </button>
            )}
          </div>
        </div>
      ))}
      <h2 className="spaced">Active matches</h2>
      {matches.map((m) => (
        <div className="operator-row" key={m.id}>
          <span>
            #{m.id.slice(0, 8)} · card {m.round + 1}
          </span>
          <button
            className="text-link"
            disabled={pending}
            onClick={() => {
              if (confirm("Abort this match? No ratings will be updated."))
                act("admin_abort_match", { match_id: m.id });
            }}
          >
            Abort match
          </button>
        </div>
      ))}
      <h2 className="spaced">Teams</h2>
      {rosterPager}
      {Array.from(new Map(bots.map((b) => [b.team_id, b])).values()).map(
        (b) => (
          <div className="operator-row" key={b.team_id}>
            <span>{b.team}</span>
            <div>
              <button
                className="text-link"
                disabled={pending}
                onClick={() => {
                  if (
                    confirm(
                      `Disable ${b.team}? Their matches will be aborted and tournaments cancelled.`,
                    )
                  )
                    act("admin_disable_team", {
                      team_id: b.team_id,
                      disabled: true,
                    });
                }}
              >
                Disable
              </button>
              <button
                className="text-link"
                disabled={pending}
                onClick={() =>
                  act("admin_disable_team", {
                    team_id: b.team_id,
                    disabled: false,
                  })
                }
              >
                Enable
              </button>
            </div>
          </div>
        ),
      )}
    </section>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
